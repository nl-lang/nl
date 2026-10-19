# The locals the inline checks read (`rt.type`, `rt.getmetatable`, `rt.KW`,
# copied once into the main function) do not count against LuaJIT's
# limits: a function may use 56 file-level names, as before, without
# the file's locals moving to the hidden table (limits.rs). The 57 case
# is lib/limits_rt_cache57.nl, which moves (cases.rs, limits_rt_cache).
# A function that uses 55 names, and one that uses 56:
fn f1(x: Number): Number x + 1 end
fn f2(x: Number): Number x + 2 end
fn f3(x: Number): Number x + 3 end
fn f4(x: Number): Number x + 4 end
fn f5(x: Number): Number x + 5 end
fn f6(x: Number): Number x + 6 end
fn f7(x: Number): Number x + 7 end
fn f8(x: Number): Number x + 8 end
fn f9(x: Number): Number x + 9 end
fn f10(x: Number): Number x + 10 end
fn f11(x: Number): Number x + 11 end
fn f12(x: Number): Number x + 12 end
fn f13(x: Number): Number x + 13 end
fn f14(x: Number): Number x + 14 end
fn f15(x: Number): Number x + 15 end
fn f16(x: Number): Number x + 16 end
fn f17(x: Number): Number x + 17 end
fn f18(x: Number): Number x + 18 end
fn f19(x: Number): Number x + 19 end
fn f20(x: Number): Number x + 20 end
fn f21(x: Number): Number x + 21 end
fn f22(x: Number): Number x + 22 end
fn f23(x: Number): Number x + 23 end
fn f24(x: Number): Number x + 24 end
fn f25(x: Number): Number x + 25 end
fn f26(x: Number): Number x + 26 end
fn f27(x: Number): Number x + 27 end
fn f28(x: Number): Number x + 28 end
fn f29(x: Number): Number x + 29 end
fn f30(x: Number): Number x + 30 end
fn f31(x: Number): Number x + 31 end
fn f32(x: Number): Number x + 32 end
fn f33(x: Number): Number x + 33 end
fn f34(x: Number): Number x + 34 end
fn f35(x: Number): Number x + 35 end
fn f36(x: Number): Number x + 36 end
fn f37(x: Number): Number x + 37 end
fn f38(x: Number): Number x + 38 end
fn f39(x: Number): Number x + 39 end
fn f40(x: Number): Number x + 40 end
fn f41(x: Number): Number x + 41 end
fn f42(x: Number): Number x + 42 end
fn f43(x: Number): Number x + 43 end
fn f44(x: Number): Number x + 44 end
fn f45(x: Number): Number x + 45 end
fn f46(x: Number): Number x + 46 end
fn f47(x: Number): Number x + 47 end
fn f48(x: Number): Number x + 48 end
fn f49(x: Number): Number x + 49 end
fn f50(x: Number): Number x + 50 end
fn f51(x: Number): Number x + 51 end
fn f52(x: Number): Number x + 52 end
fn f53(x: Number): Number x + 53 end
fn f54(x: Number): Number x + 54 end
fn f55(x: Number): Number x + 55 end
fn f56(x: Number): Number x + 56 end
# Uses 55 file-level functions, Lua's `type` and the keyed-arguments mark.
fn total55(n: Number, untyped: any): Number
  f1(n) + f2(n) + f3(n) + f4(n) + f5(n) + f6(n) + f7(n) + f8(n) + f9(n) + f10(n) + f11(n) + f12(n) + f13(n) + f14(n) + f15(n) + f16(n) + f17(n) + f18(n) + f19(n) + f20(n) + f21(n) + f22(n) + f23(n) + f24(n) + f25(n) + f26(n) + f27(n) + f28(n) + f29(n) + f30(n) + f31(n) + f32(n) + f33(n) + f34(n) + f35(n) + f36(n) + f37(n) + f38(n) + f39(n) + f40(n) + f41(n) + f42(n) + f43(n) + f44(n) + f45(n) + f46(n) + f47(n) + f48(n) + f49(n) + f50(n) + f51(n) + f52(n) + f53(n) + f54(n) + f55(n)
end
print total55(1, nil)
# Uses 56 file-level functions, Lua's `type` and the keyed-arguments mark.
fn total56(n: Number, untyped: any): Number
  f1(n) + f2(n) + f3(n) + f4(n) + f5(n) + f6(n) + f7(n) + f8(n) + f9(n) + f10(n) + f11(n) + f12(n) + f13(n) + f14(n) + f15(n) + f16(n) + f17(n) + f18(n) + f19(n) + f20(n) + f21(n) + f22(n) + f23(n) + f24(n) + f25(n) + f26(n) + f27(n) + f28(n) + f29(n) + f30(n) + f31(n) + f32(n) + f33(n) + f34(n) + f35(n) + f36(n) + f37(n) + f38(n) + f39(n) + f40(n) + f41(n) + f42(n) + f43(n) + f44(n) + f45(n) + f46(n) + f47(n) + f48(n) + f49(n) + f50(n) + f51(n) + f52(n) + f53(n) + f54(n) + f55(n) + f56(n)
end
print total56(1, nil)
