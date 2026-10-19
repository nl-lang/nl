//! Host types, types from NL, methods, member calls, ranges, keyed
//! arguments.

use nl::prelude::*;

#[derive(Default)]
struct Unit {
    hp: f64,
}

impl UserData for Unit {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("hp", |_, unit| Ok(unit.hp));
    }
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("damage", |_, unit, amount: f64| {
            if amount < 0.0 {
                return Err(Exception::new("TypeError", "amount must not be negative").into());
            }
            unit.hp -= amount;
            Ok(())
        });
    }
}

/// A type without Default.
struct Texture(u32);

impl UserData for Texture {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("id", |_, t, ()| Ok(t.0));
    }
}

fn with_decl(lua: &Lua, name: &str, decl: &str) -> LuaResult<()> {
    let dir = nl_host::tmpdir(name);
    std::fs::write(dir.join("host.d.nl"), decl).expect("write decl");
    lua.add_path(&dir)
}

#[test]
fn register_type_makes_unit_damage_and_is_work() -> LuaResult<()> {
    let lua = nl::new()?;
    with_decl(&lua, "register", "declare type Unit\n  fn damage(self, amount: Number)\nend\ndeclare type Texture\n  fn id(self): Number\nend\n")?;
    lua.register_type::<Unit>("Unit")?;
    let first = lua.register_type_with("Texture", Texture(9))?;
    let src = r##"
pub fn hit(u: Unit)
  u.damage 10
  u.hp
end
pub fn check(x) x is Unit end
pub fn name(x) typeof(x) end
pub fn tex(t: Texture) t.id end
"##;
    let m: Table = lua.load_nl(src, "@units.nl")?.try_call(())?;
    let unit = lua.create_userdata(Unit { hp: 25.0 })?;
    assert_eq!(m.get::<Function>("hit")?.try_call::<f64>(&unit)?, 15.0);
    assert!(m.get::<Function>("check")?.try_call::<bool>(&unit)?);
    assert!(!m.get::<Function>("check")?.try_call::<bool>(5)?);
    assert_eq!(m.get::<Function>("name")?.try_call::<String>(&unit)?, "Unit");
    assert_eq!(lua.type_of(&Value::UserData(unit.clone()))?, "Unit");
    assert_eq!(m.get::<Function>("tex")?.try_call::<u32>(first)?, 9);
    assert_eq!(m.get::<Function>("tex")?.try_call::<u32>(lua.create_userdata(Texture(3))?)?, 3);

    // A parameter annotated Unit checks its argument.
    let err = m.get::<Function>("hit")?.try_call::<f64>(5).unwrap_err();
    let e = err.exception().expect("TypeError");
    assert_eq!(e.type_name(), "TypeError");
    assert!(e.message().contains("Unit"), "{}", e.message());

    // A Rust exception from the method keeps its type.
    let neg = lua.load_nl("pub fn neg(u: Unit) u.damage(-1) end", "@neg.nl")?.try_call::<Table>(())?;
    let err = neg.get::<Function>("neg")?.try_call::<()>(&unit).unwrap_err();
    assert_eq!(err.exception().map(|e| e.type_name().to_owned()).as_deref(), Some("TypeError"));

    // The host type is known by name.
    let t = lua.nl_type("Unit")?.expect("Unit is registered");
    assert!(t.has(&Value::UserData(unit)));
    assert_eq!(t.name()?, "Unit");
    Ok(())
}

#[test]
fn without_register_type_the_userdata_keeps_lua_conventions() -> LuaResult<()> {
    let lua = nl::new()?;
    let unit = lua.create_userdata(Unit { hp: 5.0 })?;
    let m: Table = lua.load_nl("pub fn f(u) u:damage(1) end\npub fn t(u) typeof(u) end", "=plain")?.try_call(())?;
    m.get::<Function>("f")?.try_call::<()>(&unit)?;
    assert_eq!(unit.borrow::<Unit>()?.hp, 4.0);
    assert_eq!(m.get::<Function>("t")?.try_call::<String>(&unit)?, "Userdata");
    Ok(())
}

#[test]
fn types_from_nl() -> LuaResult<()> {
    let lua = nl::new()?;
    let src = r##"
pub struct Player
  name: String
  hp: Number = 100
end
pub enum Color
  Red
  Green
end
pub interface Named
  fn title: String
end
pub module Tools
  fn hammer
    1
  end
end
"##;
    let m: Table = lua.load_nl(src, "=game")?.try_call(())?;
    let player: Type = m.get("Player")?;
    assert_eq!(player.name()?, "Player");
    let p = player.new(lua.create_table_from([("name", "ann")])?)?;
    assert_eq!(p.get::<f64>("hp")?, 100.0);
    assert!(player.has(&Value::Table(p.clone())));
    assert!(!player.has(&Value::Number(1.0)));
    assert_eq!(lua.type_of(&Value::Table(p))?, "Player");
    let err = player.new(lua.create_table_from([("name", 5)])?).unwrap_err();
    assert_eq!(err.exception().map(|e| e.type_name().to_owned()).as_deref(), Some("TypeError"));

    let color: Type = m.get("Color")?;
    let red: Value = color.as_table().get("Red")?;
    assert!(color.has(&red));
    assert!(!color.has(&Value::Boolean(true)));
    assert_eq!(color.name()?, "Color");

    // nl_typename: every kind of type, and an error for a table that is not
    // one.
    assert_eq!(m.get::<Type>("Named")?.name()?, "Named");
    let tools: Table = m.get("Tools")?;
    assert!(Type::from_lua(Value::Table(tools), &lua)?.name().is_err());
    assert!(Type::from_lua(Value::Table(lua.create_table()?), &lua)?.name().is_err());
    assert_eq!(lua.nl_type("Task")?.expect("built-in").name()?, "Task");

    let exc = lua.nl_type("Exception")?.expect("built-in");
    assert_eq!(exc.name()?, "Exception");
    assert!(lua.nl_type("NoSuchType")?.is_none());
    Ok(())
}

#[test]
fn call_member_follows_the_method_rule() -> LuaResult<()> {
    let lua = nl::new()?;
    let count = lua.create_function(|_, args: LuaMultiValue| Ok(args.len()))?;
    let marked = lua.create_function(|_, args: LuaMultiValue| Ok(args.len()))?;
    assert!(!marked.is_method());
    marked.mark_method()?;
    assert!(marked.is_method());
    assert!(!count.is_method());
    let obj = lua.create_table()?;
    obj.set("plain", count)?;
    obj.set("method", marked)?;
    // A plain function: call_member passes no receiver, call_method does.
    assert_eq!(obj.call_member::<usize>("plain", (1, 2))?, 2);
    assert_eq!(obj.call_method::<usize>("plain", (1, 2))?, 3);
    // A method: both pass it.
    assert_eq!(obj.call_member::<usize>("method", (1, 2))?, 3);
    assert_eq!(obj.call_method::<usize>("method", (1, 2))?, 3);
    // NL sees the mark too.
    let m: Table = lua.load_nl("pub fn f(o) [o.plain(1), o.method(1)] end", "=mark")?.try_call(())?;
    let r: Table = m.get::<Function>("f")?.try_call(&obj)?;
    assert_eq!((r.get::<usize>(1)?, r.get::<usize>(2)?), (1, 2));

    // On userdata of a registered type every function is a method.
    lua.register_type::<Unit>("Unit")?;
    let unit = lua.create_userdata(Unit { hp: 3.0 })?;
    unit.call_member::<()>("damage", 1.0)?;
    assert_eq!(unit.borrow::<Unit>()?.hp, 2.0);
    // An error in the member keeps its exception.
    let err = obj.call_member::<()>("missing", ()).unwrap_err();
    assert!(err.exception().is_some(), "{err}");
    Ok(())
}

#[test]
fn ranges_and_keyed_arguments() -> LuaResult<()> {
    let lua = nl::new()?;
    let src = r##"
pub fn incl() 1..5 end
pub fn excl() 1...5 end
pub fn count(r)
  let n = 0
  for i in r
    n += 1
  end
  n
end
pub fn opts(a, **o) [a, o.x, o.y] end
"##;
    let m: Table = lua.load_nl(src, "=ranges")?.try_call(())?;
    let r: Range = m.get::<Function>("incl")?.try_call(())?;
    assert_eq!(r, Range { first: 1.0, last: 5.0, exclusive: false });
    let r: Range = m.get::<Function>("excl")?.try_call(())?;
    assert_eq!(r, Range::from(1.0..5.0));
    assert_eq!(Range::from(1.0..=5.0), Range { first: 1.0, last: 5.0, exclusive: false });
    let count: Function = m.get("count")?;
    assert_eq!(count.try_call::<f64>(Range::from(1.0..=3.0))?, 3.0);
    assert_eq!(count.try_call::<f64>(Range::from(1.0..3.0))?, 2.0);
    assert!(Range::from_lua(Value::Number(1.0), &lua).is_err());

    let opts = lua.create_table()?;
    opts.set("x", 5)?;
    opts.set("y", "why")?;
    let r: Table = m.get::<Function>("opts")?.try_call((1, lua.keyed(opts)?))?;
    assert_eq!((r.get::<f64>(1)?, r.get::<f64>(2)?, r.get::<String>(3)?), (1.0, 5.0, "why".to_owned()));
    Ok(())
}
