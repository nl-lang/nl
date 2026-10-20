//! The prelude: names every NL file can use without declaring them.
//!
//! Each name says how generated code reaches its value, through [`Access`].
//! The table is the single place to add a prelude name.

/// How the value of a prelude name is reached at runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// The Lua global of this name (`math` for `Number`, `print`).
    LuaGlobal(&'static str),
    /// The field of this name in the runtime table `rt` (`rt.typeof`,
    /// `rt.Exception`).
    Runtime(&'static str),
    /// A type that exists only for checks (`Bool`, `Function`, `any` …):
    /// the field of this name in `rt.types`.
    TypeOnly(&'static str),
}

/// What a prelude name is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreludeClass {
    /// A value: a library table or a function.
    Value,
    /// A type usable in annotations and `is` (`Number`, `Exception`, …).
    /// Library tables that are also types (`Number`, `String`, `Table`) are
    /// [`PreludeClass::ValueAndType`].
    Type,
    ValueAndType,
    /// A struct type that can be constructed (`Exception { … }`).
    Struct,
    /// An interface (`To`, `From`).
    Interface,
}

/// One prelude name.
#[derive(Debug, PartialEq, Eq)]
pub struct Prelude {
    pub name: &'static str,
    pub access: Access,
    pub class: PreludeClass,
}

impl Prelude {
    /// Usable as a value.
    pub fn is_value(&self) -> bool {
        matches!(self.class, PreludeClass::Value | PreludeClass::ValueAndType | PreludeClass::Struct)
    }

    /// Usable as a type.
    pub fn is_type(&self) -> bool {
        !matches!(self.class, PreludeClass::Value)
    }

    /// `Number`, `String` or `Bool`: a primitive type that may get methods.
    pub fn is_primitive(&self) -> bool {
        matches!(self.name, "Number" | "String" | "Bool")
    }

    /// A library that `fn Owner.name` extends: the primitives', and
    /// `Table`, whose functions are methods of the values typed as tables
    /// (resolved from the type: plain tables share no metatable).
    pub fn takes_methods(&self) -> bool {
        self.is_primitive() || self.name == "Table"
    }
}

use Access::*;
use PreludeClass::*;

macro_rules! prelude {
    ($( $name:literal => $access:expr, $class:expr; )*) => {
        static PRELUDE: &[Prelude] = &[ $( Prelude { name: $name, access: $access, class: $class }, )* ];
    };
}

prelude! {
    // NL names of Lua's library tables. `String` and `Table` are NL's
    // own string and table libraries, new Lua globals the runtime
    // installs; `Table` holds Lua's `table` functions too.
    "Number" => LuaGlobal("math"), ValueAndType;
    "String" => Runtime("String"), ValueAndType;
    "Table" => Runtime("Table"), ValueAndType;
    "IO" => LuaGlobal("io"), Value;
    "OS" => LuaGlobal("os"), Value;
    "Coroutine" => LuaGlobal("coroutine"), Value;
    "Debug" => LuaGlobal("debug"), Value;
    "Bit" => LuaGlobal("bit"), Value;
    "Package" => LuaGlobal("package"), Value;
    "Jit" => LuaGlobal("jit"), Value;
    "Event" => Runtime("Event"), Value;
    "Worker" => Runtime("Worker"), Value;
    // Library tables that are also types.
    "Task" => Runtime("Task"), ValueAndType;
    "Regex" => Runtime("Regex"), ValueAndType;
    "Time" => Runtime("Time"), ValueAndType;
    "Duration" => Runtime("Duration"), ValueAndType;
    // The NL prelude functions.
    "typeof" => Runtime("typeof"), Value;
    "raise" => Runtime("raise"), Value;
    "include" => Runtime("include"), Value;
    "add_path" => Runtime("add_path"), Value;
    "load_nl" => Runtime("load_nl"), Value;
    "keyed" => Runtime("keyed_copy"), Value;
    // Lua's free functions keep their names. `type` is a
    // keyword; Lua's is reached as `:type(x)`.
    "print" => LuaGlobal("print"), Value;
    "tostring" => LuaGlobal("tostring"), Value;
    "tonumber" => LuaGlobal("tonumber"), Value;
    "pairs" => LuaGlobal("pairs"), Value;
    "ipairs" => LuaGlobal("ipairs"), Value;
    "next" => LuaGlobal("next"), Value;
    "select" => LuaGlobal("select"), Value;
    "unpack" => LuaGlobal("unpack"), Value;
    "require" => LuaGlobal("require"), Value;
    "pcall" => LuaGlobal("pcall"), Value;
    "xpcall" => LuaGlobal("xpcall"), Value;
    "error" => LuaGlobal("error"), Value;
    "assert" => LuaGlobal("assert"), Value;
    "rawget" => LuaGlobal("rawget"), Value;
    "rawset" => LuaGlobal("rawset"), Value;
    "rawequal" => LuaGlobal("rawequal"), Value;
    "setmetatable" => LuaGlobal("setmetatable"), Value;
    "getmetatable" => LuaGlobal("getmetatable"), Value;
    "loadstring" => LuaGlobal("loadstring"), Value;
    "load" => LuaGlobal("load"), Value;
    "loadfile" => LuaGlobal("loadfile"), Value;
    "dofile" => LuaGlobal("dofile"), Value;
    "collectgarbage" => LuaGlobal("collectgarbage"), Value;
    "getfenv" => LuaGlobal("getfenv"), Value;
    "setfenv" => LuaGlobal("setfenv"), Value;
    "gcinfo" => LuaGlobal("gcinfo"), Value;
    "newproxy" => LuaGlobal("newproxy"), Value;
    // Lua's runtime types that have no library table.
    "Bool" => TypeOnly("Bool"), Type;
    "Function" => TypeOnly("Function"), Type;
    "Userdata" => TypeOnly("Userdata"), Type;
    "Thread" => TypeOnly("Thread"), Type;
    // The exception hierarchy.
    "Exception" => Runtime("Exception"), Struct;
    "LuaError" => Runtime("LuaError"), Struct;
    "TypeError" => Runtime("TypeError"), Struct;
    "MatchError" => Runtime("MatchError"), Struct;
    "ConversionError" => Runtime("ConversionError"), Struct;
    "RegexError" => Runtime("RegexError"), Struct;
    "EncodingError" => Runtime("EncodingError"), Struct;
    "KeyError" => Runtime("KeyError"), Struct;
    "SendError" => Runtime("SendError"), Struct;
    "WorkerError" => Runtime("WorkerError"), Struct;
    // `To<T>` and `From<T>` are taken to be prelude interfaces
    // (`conv_custom.nl` assumes so).
    "To" => Runtime("To"), Interface;
    "From" => Runtime("From"), Interface;
}

/// The prelude entry of a name, if any.
pub fn lookup(name: &str) -> Option<&'static Prelude> {
    PRELUDE.iter().find(|p| p.name == name)
}

/// Every prelude name, in the order of the table (for completion in the
/// language server).
pub fn all() -> &'static [Prelude] {
    PRELUDE
}

/// Lua globals that are not NL names, with the NL way to reach them, for
/// the "undeclared name" message.
pub fn lua_library_hint(name: &str) -> Option<&'static str> {
    Some(match name {
        "math" => "`math` is not an NL name; use `Number.floor(x)`, `x.floor` or the Lua path `:math.floor(x)`",
        "string" => "Lua's `string` is visible in NL files only as the Lua path `:string`; NL's string library is `String`",
        "table" => "`table` is not an NL name; use `Table` or the Lua path `:table`",
        "io" => "`io` is not an NL name; use `IO` or the Lua path `:io`",
        "os" => "`os` is not an NL name; use `OS` or the Lua path `:os`",
        "coroutine" => "`coroutine` is not an NL name; use `Coroutine` or the Lua path `:coroutine`",
        "debug" => "`debug` is not an NL name; use `Debug` or the Lua path `:debug`",
        "bit" => "`bit` is not an NL name; use `Bit` or the Lua path `:bit`",
        "package" => "`package` is not an NL name; use `Package` or the Lua path `:package`",
        "jit" => "`jit` is not an NL name; use `Jit` or the Lua path `:jit`",
        "event" => "`event` is not an NL name; use `Event` or the Lua path `:event`",
        "worker" => "`worker` is not an NL name; use `Worker` or the Lua path `:worker`",
        "nl" => "there is no `nl` table; the prelude functions are plain globals",
        _ => return None,
    })
}
