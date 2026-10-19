# The Lua and LuaJIT standard libraries under their NL names,
# for the analyzer. Built into the compiler (crates/nl_sema/src/decl.rs);
# the resolver already knows these names as prelude names, so only the
# types here matter, and the Lua paths repeat what the prelude emits.
#
# Where Lua is polymorphic or returns several values of unknown kinds, a
# parameter or result is `any`: the analyzer accepts, and the runtime
# decides. Functions of `Number` and `String` are also methods
# of numbers and strings: the receiver is the first argument.

declare module Number = :math
  const pi: Number
  const huge: Number
  fn abs(x: Number): Number
  fn ceil(x: Number): Number
  fn floor(x: Number): Number
  fn max(x: Number, *more: [Number]): Number
  fn min(x: Number, *more: [Number]): Number
  fn sqrt(x: Number): Number
  fn pow(x: Number, y: Number): Number
  fn exp(x: Number): Number
  fn log(x: Number, base: Number?): Number
  fn log10(x: Number): Number
  fn sin(x: Number): Number
  fn cos(x: Number): Number
  fn tan(x: Number): Number
  fn asin(x: Number): Number
  fn acos(x: Number): Number
  fn atan(x: Number): Number
  fn atan2(y: Number, x: Number): Number
  fn sinh(x: Number): Number
  fn cosh(x: Number): Number
  fn tanh(x: Number): Number
  fn fmod(x: Number, y: Number): Number
  fn modf(x: Number): (Number, Number)
  fn frexp(x: Number): (Number, Number)
  fn ldexp(m: Number, e: Number): Number
  fn deg(x: Number): Number
  fn rad(x: Number): Number
  fn random(m: Number?, n: Number?): Number
  fn randomseed(x: Number)
  ## `n.chr`: the string for a code point; stored in `math`.
  fn chr(n: Number): String
end

declare module Table = :table
  fn insert(t: Table, pos_or_value: any, value: any?)
  fn remove(t: Table, pos: Number?): any
  fn concat(t: Table, sep: String?, i: Number?, j: Number?): String
  fn sort(t: Table, comp: Function?)
  fn maxn(t: Table): Number
  fn getn(t: Table): Number
end

## A Lua file handle (`io.open`, `IO.stdout`), a host type the runtime
## marks: its methods work with `.` as well as `:`.
declare type File
  fn write(self, *args: any): File
  fn read(self, *formats: any): any
  fn lines(self, *formats: any): Function
  fn close(self): Bool
  fn flush(self): File
  fn seek(self, whence: String?, offset: Number?): Number
  fn setvbuf(self, mode: String, size: Number?): Bool
end

declare module IO = :io
  stdin: File
  stdout: File
  stderr: File
  fn write(*args: any): File
  fn read(*formats: any): any
  fn lines(filename: String?, *formats: any): Function
  fn open(filename: String, mode: String?): (File?, String?)
  fn close(file: File?): Bool
  fn input(file: any?): File
  fn output(file: any?): File
  fn popen(prog: String, mode: String?): (File?, String?)
  fn tmpfile(): File?
  fn type(obj: any): String?
end

declare module OS = :os
  fn time(t: Table?): Number
  fn clock(): Number
  fn date(format: String?, time: Number?): any
  fn difftime(t2: Number, t1: Number): Number
  fn getenv(name: String): String?
  fn exit(code: any?): never
  fn remove(filename: String): (Bool?, String?)
  fn rename(old: String, new: String): (Bool?, String?)
  fn tmpname(): String
  fn execute(command: String?): Number
  fn setlocale(locale: String?, category: String?): String?
end

declare module Coroutine = :coroutine
  fn create(f: Function): Thread
  fn resume(co: Thread, *args: any): (Bool, any)
  fn yield(*args: any): any
  fn status(co: Thread): String
  fn wrap(f: Function): Function
  fn running(): Thread?
  fn isyieldable(): Bool
end

declare module Debug = :debug
  fn traceback(*args: any): any
  fn getinfo(*args: any): Table?
  fn getmetatable(value: any): Table?
  fn setmetatable(value: any, mt: Table?): any
  fn getlocal(*args: any): (String?, any)
  fn setlocal(*args: any): String?
  fn getupvalue(f: Function, up: Number): (String?, any)
  fn setupvalue(f: Function, up: Number, value: any): String?
  fn getfenv(o: any): Table
  fn setfenv(o: any, env: Table): any
  fn sethook(*args: any)
  fn gethook(*args: any): any
  fn debug()
end

declare module Bit = :bit
  fn band(*args: Number): Number
  fn bor(*args: Number): Number
  fn bxor(*args: Number): Number
  fn bnot(x: Number): Number
  fn lshift(x: Number, n: Number): Number
  fn rshift(x: Number, n: Number): Number
  fn arshift(x: Number, n: Number): Number
  fn rol(x: Number, n: Number): Number
  fn ror(x: Number, n: Number): Number
  fn bswap(x: Number): Number
  fn tobit(x: Number): Number
  fn tohex(x: Number, n: Number?): String
end

declare module Package = :package
  loaded: Table
  preload: Table
  loaders: Table
  path: String
  cpath: String
  fn searchpath(name: String, path: String, sep: String?, rep: String?): (String?, String?)
  fn loadlib(libname: String, funcname: String): any
  fn seeall(mod: Table)
end

declare module Jit = :jit
  const version: String
  const version_num: Number
  const os: String
  const arch: String
  opt: Table
  fn on(*args: any)
  fn off(*args: any)
  fn flush(*args: any)
  fn status(): (Bool, any)
  fn attach(handler: Function, event: String?)
end

# Lua's free functions keep their names; they are variables, not
# function-kind names. These signatures type their calls.
declare fn print(*args: any)
declare fn tostring(v: any): String
declare fn tonumber(v: any, base: Number?): Number?
declare fn pairs(t: any): (Function, any, any)
declare fn ipairs(t: any): (Function, any, any)
declare fn next(t: Table, key: any?): (any, any)
declare fn select(n: Number | String, *args: any): any
declare fn unpack(t: Table, i: Number?, j: Number?): any
declare fn require(name: String): any
declare fn pcall(f: Function, *args: any): (Bool, any)
declare fn xpcall(f: Function, handler: Function, *args: any): (Bool, any)
declare fn error(value: any, level: Number?): never
declare fn assert(v: any, message: any?): any
declare fn rawget(t: Table, key: any): any
declare fn rawset(t: Table, key: any, value: any): Table
declare fn rawequal(a: any, b: any): Bool
declare fn setmetatable(t: Table, mt: Table?): Table
declare fn getmetatable(v: any): any
declare fn loadstring(source: String, name: String?): (Function?, String?)
declare fn load(chunk: any, name: String?): (Function?, String?)
declare fn loadfile(filename: String?): (Function?, String?)
declare fn dofile(filename: String?): any
declare fn collectgarbage(opt: String?, arg: Number?): any
declare fn getfenv(f: any?): Table
declare fn setfenv(f: any, env: Table): Function?
declare fn gcinfo(): Number
declare fn newproxy(v: any?): Userdata
