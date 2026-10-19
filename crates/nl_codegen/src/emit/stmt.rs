//! Statements and the chunk (the `parse_*` statement functions of
//! `src/lj_parse.c`).

use nl_ir::ir::*;

use super::{Exp, Gen, Kind, R, Target, VarId, assigned_locals};
use crate::bc::{self, NO_JMP, Op};
use crate::proto::{self, Proto, VarName};

/// Compiles a chunk to the prototype of its loader `P` (see the module
/// documentation of [`super`]).
pub(crate) fn compile_chunk(chunk: &Chunk, level: u8) -> R<Proto> {
    let mut g = Gen { chunk, funcs: Vec::new(), assigned: assigned_locals(chunk), line: 0, level, want_multi: false };
    g.func_begin("the chunk loader".to_owned(), 0, true, false, &[])?;
    g.fs().has_closures = true;
    // local rt = ...
    g.local_stmt(&[VarId::Rt], &[Expr::Vararg])?;
    if let Some(owner) = chunk.owner {
        // local owner = rt.owner("@name")
        g.local_stmt(&[VarId::Local(owner)], &[Expr::intrinsic_call("owner", vec![Expr::str(&chunk.name)])])?;
    }
    // return function(...) <main> end
    let mut main = g.closure(&chunk.main)?;
    let r = g.toanyreg(&mut main)?;
    g.emit_aj(Op::UCLO, 0, 0)?;
    g.emit_ad(Op::RET1, r, 2)?;
    g.func_end()
}

impl Gen<'_> {
    /// The statements of a block in the current scope.
    pub(crate) fn stmts(&mut self, b: &Block) -> R<()> {
        for s in &b.stmts {
            self.stmt(s)?;
        }
        Ok(())
    }

    /// A block in a scope of its own (`parse_block`).
    pub(crate) fn block(&mut self, b: &Block) -> R<()> {
        self.scope_begin(false);
        self.stmts(b)?;
        self.scope_end()
    }

    fn stmt(&mut self, s: &Stmt) -> R<()> {
        self.line = s.line;
        match &s.kind {
            StmtKind::Local { locals, values } => {
                let ids: Vec<VarId> = locals.iter().map(|l| VarId::Local(*l)).collect();
                self.local_stmt(&ids, values)?;
            }
            StmtKind::Assign { targets, values } => self.assign(targets, values)?,
            StmtKind::Expr(e) => {
                let mut v = self.expr(e)?;
                match v.k {
                    Kind::Call(pc, _) => self.patch_ins(pc, |i| bc::set_b(i, 1)),
                    _ => self.tonextreg(&mut v)?,
                }
            }
            StmtKind::Do(b) => self.block(b)?,
            StmtKind::If { cond, then, otherwise } => self.if_stmt(cond, then, otherwise.as_ref())?,
            StmtKind::While { cond, body } => self.while_stmt(cond, body)?,
            StmtKind::NumericFor { var, start, limit, step, body } => self.numeric_for(*var, start, limit, step.as_ref(), body)?,
            StmtKind::GenericFor { vars, iter, body } => self.generic_for(vars, iter, body)?,
            StmtKind::Return(vals) => self.return_stmt(vals)?,
            StmtKind::Break => {
                let pc = self.emit_jmp()?;
                self.goto_new(Target::Break, pc);
            }
            StmtKind::Goto(l) => {
                let pc = self.emit_jmp()?;
                self.goto_new(Target::Label(*l), pc);
            }
            StmtKind::Label(l) => self.label_here(*l)?,
            StmtKind::Hoist { defs, rest } => {
                self.stmts(defs)?;
                self.stmts(rest)?;
            }
        }
        // Free the temporaries of the statement.
        let fs = self.fs();
        debug_assert!(fs.framesize >= fs.freereg && fs.freereg >= fs.nactvar, "bad register state after a statement");
        fs.freereg = fs.nactvar;
        Ok(())
    }

    /// `local a, b = values` (`parse_local`).
    pub(crate) fn local_stmt(&mut self, ids: &[VarId], values: &[Expr]) -> R<()> {
        let nvars = u32::try_from(ids.len()).unwrap_or(u32::MAX);
        // The last value expands only when the targets outnumber the values.
        let (nexps, mut e) = if values.is_empty() { (0, Exp::new(Kind::Void)) } else { self.expr_list(values, ids.len() > values.len())? };
        self.assign_adjust(nvars, nexps, &mut e)?;
        for id in ids {
            self.var_add_id(*id)?;
        }
        Ok(())
    }

    /// Fits `nexps` values to `nvars` targets (`assign_adjust`): a call
    /// gives the missing values, or nils fill in.
    fn assign_adjust(&mut self, nvars: u32, nexps: u32, e: &mut Exp) -> R<()> {
        let mut extra = i64::from(nvars) - i64::from(nexps);
        if let Kind::Call(pc, _) = e.k {
            extra += 1;
            if extra < 0 {
                extra = 0;
            }
            let b = (extra + 1) as u32;
            self.patch_ins(pc, |i| bc::set_b(i, b));
            if extra > 1 {
                self.reserve(extra as u32 - 1)?;
            }
        } else {
            if e.k != Kind::Void {
                self.tonextreg(e)?;
            }
            if extra > 0 {
                let reg = self.fs().freereg;
                self.reserve(extra as u32)?;
                self.emit_nil(reg, extra as u32)?;
            }
        }
        if nexps > nvars {
            self.fs().freereg -= nexps - nvars;
        }
        Ok(())
    }

    /// `targets = values` (`parse_assignment`): the targets' tables and
    /// keys are evaluated first, then the values, then the stores from
    /// the last target to the first.
    fn assign(&mut self, targets: &[Place], values: &[Expr]) -> R<()> {
        let mut lhs: Vec<Exp> = Vec::with_capacity(targets.len());
        for t in targets {
            let v = match t {
                Place::Local(l) => {
                    let v = self.var_lookup(VarId::Local(*l))?;
                    if let Kind::Local(slot) = v.k {
                        self.assign_hazard(&mut lhs, slot)?;
                    }
                    v
                }
                Place::Global(g) => Exp::new(Kind::Global(g.as_bytes().to_vec())),
                Place::Index(o, k) => {
                    let mut t = self.expr(o)?;
                    self.toanyreg(&mut t)?;
                    let mut key = self.expr(k)?;
                    self.toval(&mut key)?;
                    self.index(&mut t, &mut key)?;
                    t
                }
            };
            lhs.push(v);
        }
        let nvars = u32::try_from(lhs.len()).unwrap_or(u32::MAX);
        let (nexps, mut e) = self.expr_list(values, lhs.len() > values.len())?;
        let Some(last) = lhs.pop() else { return Ok(()) };
        if nexps == nvars {
            if let Kind::Call(pc, base) = e.k {
                if Op::from_ins(self.ins_at(pc)) == Some(Op::VARG) {
                    self.fs().freereg -= 1;
                    e.k = Kind::Reloc(pc);
                } else {
                    e.k = Kind::NonReloc(base);
                }
            }
            self.store(&last, &mut e)?;
        } else {
            self.assign_adjust(nvars, nexps, &mut e)?;
            let r = self.fs().freereg - 1;
            let mut v = Exp::new(Kind::NonReloc(r));
            self.store(&last, &mut v)?;
        }
        while let Some(t) = lhs.pop() {
            let r = self.fs().freereg - 1;
            let mut v = Exp::new(Kind::NonReloc(r));
            self.store(&t, &mut v)?;
        }
        Ok(())
    }

    /// Copies a local that an earlier indexed target uses as its table or
    /// key before the local is assigned (`assign_hazard`): `t[i], i = …`.
    fn assign_hazard(&mut self, lhs: &mut [Exp], reg: u32) -> R<()> {
        let tmp = self.fs().freereg;
        let mut hazard = false;
        for l in lhs.iter_mut() {
            if let Kind::Indexed(tab, key) = &mut l.k {
                if *tab == reg {
                    hazard = true;
                    *tab = tmp;
                }
                if *key == super::Key::Reg(reg) {
                    hazard = true;
                    *key = super::Key::Reg(tmp);
                }
            }
        }
        if hazard {
            self.emit_ad(Op::MOV, tmp, reg)?;
            self.reserve(1)?;
        }
        Ok(())
    }

    fn if_stmt(&mut self, cond: &Expr, then: &Block, otherwise: Option<&Block>) -> R<()> {
        let mut flist = self.cond(cond)?;
        self.block(then)?;
        let mut escapelist = NO_JMP;
        let mut otherwise = otherwise;
        loop {
            match otherwise {
                // `else if` with nothing else in the block is `elseif`.
                Some(o) if matches!(o.stmts.as_slice(), [Stmt { kind: StmtKind::If { .. }, .. }]) => {
                    let Some(Stmt { line, kind: StmtKind::If { cond, then, otherwise: next } }) = o.stmts.first() else { break };
                    let j = self.emit_jmp()?;
                    self.jmp_append(&mut escapelist, j)?;
                    self.jmp_tohere(flist)?;
                    self.line = *line;
                    flist = self.cond(cond)?;
                    self.block(then)?;
                    otherwise = next.as_ref();
                }
                Some(o) => {
                    let j = self.emit_jmp()?;
                    self.jmp_append(&mut escapelist, j)?;
                    self.jmp_tohere(flist)?;
                    self.block(o)?;
                    break;
                }
                None => {
                    self.jmp_append(&mut escapelist, flist)?;
                    break;
                }
            }
        }
        self.jmp_tohere(escapelist)
    }

    fn while_stmt(&mut self, cond: &Expr, body: &Block) -> R<()> {
        let start = self.fs().pc();
        self.fs().lasttarget = start;
        let condexit = self.cond(cond)?;
        self.scope_begin(true);
        let nactvar = self.fs().nactvar;
        let loop_pc = self.emit_ad(Op::LOOP, nactvar, 0)?;
        self.block(body)?;
        let j = self.emit_jmp()?;
        self.jmp_patch(j, start)?;
        self.scope_end()?;
        self.jmp_tohere(condexit)?;
        let pc = self.fs().pc();
        self.jmp_patchins(loop_pc, pc)
    }

    /// `for var = start, limit, step` (`parse_for_num`): three hidden
    /// control slots, then the visible copy of the index.
    fn numeric_for(&mut self, var: LocalId, start: &Expr, limit: &Expr, step: Option<&Expr>, body: &Block) -> R<()> {
        let line = self.line;
        self.scope_begin(true);
        let base = self.fs().freereg;
        let mut e = self.expr(start)?;
        self.tonextreg(&mut e)?;
        let mut e = self.expr(limit)?;
        self.tonextreg(&mut e)?;
        match step {
            Some(s) => {
                let mut e = self.expr(s)?;
                self.tonextreg(&mut e)?;
            }
            None => {
                let r = self.fs().freereg;
                self.emit_ad(Op::KSHORT, r, 1)?;
                self.reserve(1)?;
            }
        }
        self.var_add(None, VarName::Internal(proto::VARNAME_FOR_IDX))?;
        self.var_add(None, VarName::Internal(proto::VARNAME_FOR_STOP))?;
        self.var_add(None, VarName::Internal(proto::VARNAME_FOR_STEP))?;
        let loop_pc = self.emit_aj(Op::FORI, base, NO_JMP)?;
        self.scope_begin(false);
        self.var_add_id(VarId::Local(var))?;
        self.reserve(1)?;
        self.block(body)?;
        self.scope_end()?;
        let loopend = self.emit_aj(Op::FORL, base, NO_JMP)?;
        self.set_line(loopend, line);
        self.jmp_patchins(loopend, loop_pc + 1)?;
        let pc = self.fs().pc();
        self.jmp_patchins(loop_pc, pc)?;
        self.scope_end()
    }

    /// `for vars in iter` (`parse_for_iter`): the generator, state and
    /// control slots, then the variables. `ISNEXT`/`ITERN` when the
    /// iterator expression starts with `pairs` or `next` (the VM checks).
    fn generic_for(&mut self, vars: &[LocalId], iter: &[Expr], body: &Block) -> R<()> {
        let line = self.line;
        self.scope_begin(true);
        let base = self.fs().freereg + 3;
        let nvars = 3 + u32::try_from(vars.len()).unwrap_or(u32::MAX);
        let exprpc = self.fs().pc();
        let (nexps, mut e) = self.expr_list(iter, true)?;
        self.assign_adjust(3, nexps, &mut e)?;
        self.bump(3 + 1)?;
        let isnext = nvars <= 5 && self.fs().pc() > exprpc && self.predict_next(exprpc);
        self.var_add(None, VarName::Internal(proto::VARNAME_FOR_GEN))?;
        self.var_add(None, VarName::Internal(proto::VARNAME_FOR_STATE))?;
        self.var_add(None, VarName::Internal(proto::VARNAME_FOR_CTL))?;
        let loop_pc = self.emit_aj(if isnext { Op::ISNEXT } else { Op::JMP }, base, NO_JMP)?;
        self.scope_begin(false);
        for v in vars {
            self.var_add_id(VarId::Local(*v))?;
        }
        self.reserve(nvars - 3)?;
        self.block(body)?;
        self.scope_end()?;
        let pc = self.fs().pc();
        self.jmp_patchins(loop_pc, pc)?;
        let call = self.emit_abc(if isnext { Op::ITERN } else { Op::ITERC }, base, nvars - 3 + 1, 2 + 1)?;
        self.set_line(call, line);
        let loopend = self.emit_aj(Op::ITERL, base, NO_JMP)?;
        self.set_line(loopend, line);
        self.jmp_patchins(loopend, loop_pc + 1)?;
        self.scope_end()
    }

    /// `predict_next`: the iterator expression starts by loading a
    /// variable named `pairs` or `next`.
    fn predict_next(&mut self, pc: u32) -> bool {
        let ins = self.ins_at(pc);
        let name: Vec<u8> = match Op::from_ins(ins) {
            Some(Op::MOV) => {
                let d = bc::d_of(ins);
                if d >= self.fs().nactvar {
                    return false;
                }
                match self.fs_ref().and_then(|f| f.local_name(d)) {
                    Some(n) => n,
                    None => return false,
                }
            }
            Some(Op::UGET) => match self.fs_ref().and_then(|f| f.upval_name(bc::d_of(ins))).map(|id| self.var_name(id)) {
                Some(n) => n,
                None => return false,
            },
            Some(Op::GGET) => match self.fs_ref().and_then(|f| f.kstr(bc::d_of(ins))) {
                Some(s) => s.to_vec(),
                None => return false,
            },
            _ => return false,
        };
        name == b"pairs" || name == b"next"
    }

    /// `return values` (`parse_return`): a single call becomes a tail
    /// call, except an intrinsic's, whose frame matters ([`return_form`]
    /// of nl_ir, as in the Lua backend): one with one result returns that
    /// value, any other all its values; a function with closures closes
    /// its upvalues first.
    fn return_stmt(&mut self, vals: &[Expr]) -> R<()> {
        let ins = match vals {
            [] => bc::ad(Op::RET0, 0, 1),
            [v] => {
                let form = return_form(v);
                self.want_multi = true;
                let mut e = self.expr(v)?;
                match e.k {
                    Kind::Call(pc, base) if form == ReturnForm::AllValues => {
                        // An ordinary call with all its results, then RETM.
                        self.patch_ins(pc, |i| bc::set_b(i, 0));
                        let nactvar = self.fs().nactvar;
                        bc::ad(Op::RETM, nactvar, base - nactvar)
                    }
                    Kind::Call(pc, base) if form == ReturnForm::Tail => {
                        let ip = self.ins_at(pc);
                        if Op::from_ins(ip) == Some(Op::VARG) {
                            // `return ...`: all the values, no tail call.
                            self.patch_ins(pc, |i| bc::set_b(i, 0));
                            let nactvar = self.fs().nactvar;
                            bc::ad(Op::RETM, nactvar, base - nactvar)
                        } else {
                            // The call is the last instruction: replace it.
                            self.fs().bc.pop();
                            self.fs().lines.pop();
                            let op = if Op::from_ins(ip) == Some(Op::CALLM) { Op::CALLMT } else { Op::CALLT };
                            bc::ad(op, bc::a_of(ip), bc::c_of(ip))
                        }
                    }
                    _ => {
                        let r = self.toanyreg(&mut e)?;
                        bc::ad(Op::RET1, r, 2)
                    }
                }
            }
            _ => {
                let (nret, mut e) = self.expr_list(vals, true)?;
                if let Kind::Call(pc, base) = e.k {
                    self.patch_ins(pc, |i| bc::set_b(i, 0));
                    let nactvar = self.fs().nactvar;
                    bc::ad(Op::RETM, nactvar, base - nactvar)
                } else {
                    self.tonextreg(&mut e)?;
                    let nactvar = self.fs().nactvar;
                    bc::ad(Op::RET, nactvar, nret + 1)
                }
            }
        };
        if self.fs().has_closures {
            self.emit_aj(Op::UCLO, 0, 0)?;
        }
        self.emit(ins)?;
        Ok(())
    }
}
