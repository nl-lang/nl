# this is a comment
# no multi-line comments allowed

# imports are the first statements of a file: only comments may come before them
# every import names what it brings in
import foo from 'filename'
import { foo, bar } from 'filename'
import from 'filename' as File # everything the file exports, as File.foo, File.bar

# all types are optional. you can omit specifying the type anytime.
# a declared type is checked at runtime (a mismatch raises an error),
# and the compiler uses declared types to work out what things are.

# variable defs
let a
const a = 1 # constants cannot be modified, so they must be given a value
global C # globals must follow PascalCase
         # globals are generally discouraged and will raise an analyzer warning
let d: Number # variable with type binding
              # in most cases, the parser will try to infer the type automatically

# basic types
let a: nil
let b: Bool
let c: Number
let d: String
let e: Table
let f: Function
let g: Userdata
let h: Thread

# meta types (only relevant for the analyzer)
let a: any
let b: never
let c: unknown

# numbers
let a = 1
let b = 1.1
let c = 0xabcdef
let d = 1_000_000
let e = 5e+20
let f = 0.3e12
let g = 4.57e-3

# math operators
let res = a + b
let res = a - b
let res = a * b
let res = a / b
let res = a // b
let res = a ** b
let res = a % b
let res = -a

# math assignment operators
a += b
a -= b
a *= b
a /= b
a //= b
a **= b
a %= b

# bitwise operators
let res = a | b
let res = a & b
let res = a >> 1
let res = a << 1
let res = a ^ b
let res = ~a

# comparison operators
let res = a > b
let res = a >= b
let res = a < b
let res = a <= b
let res = a == b
let res = a != b

# strings
let a = 'simple string'
let b = "variable a was #{a}"
let c = "concatenating strings is easy!" <> a <> b
let d = """
multi
line
string
this works here: #{a}
"""

let e = """-
  this
  string will ignore
  the first level of indentation
  this works here too: #{a}
"""

let f = """=
this won't trigger string interpolation
so #{a} will be output literally as "#{a}"
"""

let g = """-=
  the two flags can be combined:
  no indentation and no interpolation, so #{a} stays as written
"""

# tables
let a = [1, 2, 3] # table indices are 1-based, so a[1] == 1
let b = {
  a: 1, # keys like this are treated as strings
  b: 2,
  1: 3 # no need to wrap literals in []
}
let c = [
  ["table inside table"]
]

# length
# .len works on anything that has a length (Lua's # operator)
# for a string, it counts bytes

let n = a.len # 3
let n = "héllo".len # 6

# UTF-8
# strings hold bytes, but NL's own string functions understand UTF-8 and
# count in characters. they are named like Ruby's.
# Lua's string functions are still there, and still work on bytes

let s = "héllo"
s.length # 5
s[1] # "h"
s[1..2] # "hé"
s.upcase # "HÉLLO"
s.downcase # "héllo"
s.reverse # "olléh"

# these replace Lua's string functions in NL files only. Lua files still get Lua's,
# and an NL file can call those through a Lua path
:string.upper(s) # Lua's upper, which only knows ASCII

for ch in s # goes through the characters
end

for byte in s.as_bytes # goes through the bytes
end

# a string that is not valid UTF-8 raises an EncodingError in these functions.
# .len, as_bytes and valid_encoding work on any bytes

# functions

fn foo # function names must be lowercase, and contain only alphanumeric characters and underscores
  # ...
end

fn foo(a)
  # ...
end

fn foo(a: Number): nil # if one parameter has a type, all of them must have one.
                       # the return type can be omitted, and is then inferred.
                       # `: nil` means the function returns nothing, whatever its last line is
end

let a = fn print('hello world') end
let b = fn(a) print('hello world') end
let c = fn(a: Number): nil print('hello world') end

# short functions: `|params| -> expr` is `fn(params) expr end`, for small callbacks
let double = x -> x * 2 # one parameter needs no pipes
let add = |a, b| -> a + b
let ready = || -> true
let typed = |a: Number|: Number -> a * 2 # parameters as in a block: types, defaults, *args
let swap = |a, b| -> do return b, a end # several values or statements take a block body

# types

type Foo = Number # types must follow PascalCase
type Composite = Number | String # value must be a number or a string
type NegativeComposite = ~Number # value must be a type that isn't a number

# structs

struct Foo   # structs must follow PascalCase
  a: Number  # structs are also auto-registered as a type
  b: String  # one field per line, no commas
end

struct Bar : Foo    # struct Bar extends Foo
  c: Table          # struct can only extend one other struct
end                 # extended struct cannot change parent struct's types

fn foo(a: Foo) # would accept an argument of type Foo, or derivatives, like Bar
end

let a = Foo { a: 1, b: "x" } # creates a variable with a struct

# structs act as a substitute for classes
# to define struct methods simply just define a function
# under the struct

fn Foo.method
  print "Foo values: #{@a} #{@b}" # use @ to access struct properties within struct methods
  print "Foo values: #{self.a} #{self.b}" # self may also be used
end

# all functions defined on structs and modules have `self` as implicit first argument

# Self (capitalized) means "this struct" in a struct function,
# and "this module" inside a module

fn Foo.create(a: Number): Self
  Self { a: a, b: "x" }
end

# to call a struct method:
a.method

# interfaces

interface Foo             # one member per line
  fn a                    # any struct implementing the interface
  fn b(var)               # must implement all the interface's functions
  fn c(var: Number): nil  # just like Rust traits
  d: String               # a member without fn is a field the struct must have
end

# to implement an interface on struct

interface Foo for SomeStruct
  fn a
  end

  fn b(var)
  end

  fn c(var: Number): nil
    nil
  end
end

# events

on EventName do
  # ...
end

on EventName do |arg1, arg2|
  # ...
end

on EventName do |arg1: Number, arg2: String|: nil
  # ...
end

# this is syntax sugar for the Event library, which is built into the VM:
#
# Event.on_run(name, handler_name, callback) adds a handler to an event
# Event.remove(name, handler_name) removes it
# Event.run(name, ...) runs the handlers of an event
#
# `on EventName do ... end` calls Event.on_run with the path of the file as
# the handler name, so a file can have one such handler per event.
# to pick the handler name, add `as`:

on EventName as 'handler_name' do
  # ...
end

Event.remove "EventName", "handler_name"

# Event.run passes its arguments to the handlers, and returns whatever a handler returns
let res = Event.run "EventName", 123, "abc"

# This is included in the language standard because this language is
# purpose-designed for event-driven game development.

# calling lua

# Lua's standard libraries have NL names: Number (Lua's math), String, Table, OS, IO...
# their functions are called the same way Lua calls them
print Number.floor(2.5)
print 2.5.floor # the same thing: Lua's math functions are methods of numbers

# the Lua names themselves are not known to NL
# math.floor(2.5) would be an error, because there is no variable named math

# a leading : starts a Lua path. the names in it are Lua's own, so they may be
# NL keywords, or use capitalization that NL names cannot have
print :string.len("some string")
print :type(a) # Lua's type function. plain `type` is an NL keyword
:player.getHealth()
:GetPlayerName(1)
let max = :config.maxPlayers # reading works as well as calling

# there are no raw Lua blocks. Lua code goes into .lua files

# calling functions

a # calls a if it was declared as a function: with `fn a`, with a Function type
  # (`let a: Function`, a parameter `a: Function`), or with `let a = fn ... end`.
  # a name on a line of its own is always called
a() # call using () explicitly in case it's unclear if something is a function
a b # will call function a with b as argument
a b, c # will call function a with b and c as arguments
a b, c d # will raise a parser error (ambiguous function call)
a b, c(d) # is ok

# returning a function without calling it

fn foo: Function
  let a = fn end

  a! # exclamation point at the end will prevent the automatic function call
end

# callbacks
# a function that has a `Function` parameter can be called with a block, which is passed as the final argument

fn foo(a: Number, callback: Function)
  # ...
end

foo 123 do
  # contents of the callback
end

# a short function is passed like any other function value
foo 123, n -> print(n) # in a command call; `f x -> x + 1` is `f(x -> x + 1)`
:table.sort(players, |a, b| -> a.hp < b.hp) # a Lua function gets a plain function

# the Function type also accepts the function signature in the argument
# warning: no runtime checks for this, only static analysis.
fn foo(a: Function(a: Number, b: String): nil)
end

# control flow
# the first line of if, while, for etc. ends at the line break. there is no `then` or `do`

# if

if condition
  # do something
elseif condition
  # another thing
else
  # otherwise do this
end

unless condition
  # do something
end

some_function() if condition
some_function() unless condition

# case

case var
when 1
  # do something
when 2
  # do another thing
else
  # default case
end

# cond

cond
when a > b
  # do something
when a < b
  # do other thing
else
  # otherwise do this
end

# comparisons, negations, logical operators

fn foo(a, b)
  if a == b || a != b || !a || !b
  end
end

fn foo(a, b)
  if a == b or a != b or not a or not b
  end
end

fn foo(a, b, c)
  if (a || b) && c
  end
end

fn foo(a, b, c)
  if (a or b) and c
  end
end

# logical assignment

fn a(var)
  var ||= "owo"
  var &&= "uwu"
  var ??= "only when nil" # unlike ||=, keeps false
end

# nil-safe operators
# a?.b is nil when a is nil, and then the rest of the chain is skipped:
# here neither .city nor .upcase runs when there is no user or no address

fn city_of(user)
  user?.address?.city.upcase ?? "unknown"
end

let first = list?.[1]
let reply = handler?.(request) # handler is called only when it is not nil
let pos = :LocalPlayer()?.:GetPos() # Lua's method call, only on a value that is not nil
settings?.volume = 5 # nothing is stored, and 5 not evaluated, when settings is nil

# pipe
# elixir-like "put the thing on the left as the first argument to the function call on the right"

let a = "string"

# the two examples below are equivalent:
some_func a
a |> some_func

a
|> some_func_that_returns_something
|> func_that_takes_previous_funcs_return_value

# return

fn foo
  nil # the final statement of each control flow branch will automatically return
end

fn foo
  return nil
end

fn foo: Bool
  if a
    true
  else
    false
  end
end

# this will raise an error
fn foo: Bool
  if a
    true
  end

  false
end

# ...and hence must be written as
fn foo: Bool
  if a
    return true
  end

  false
end

# modules

module Foo # module names must follow PascalCase
  fn some_func
  end
end

Foo.some_func

module Foo.Bar
  fn another_func
  end
end

# it is possible to add a function to a module later on by using the module statement again

module Foo
  fn additional_func
  end
end

# simple scope

do
  let a = 123
  # other stuff
end
# a is out of scope here

# type checks

if a is Number
  # stuff to do if a is a number
end

# `type` is an NL keyword, so Lua's built in "type" function is called as `:type(a)`
# to get the NL-specific type use `typeof` instead

# type conversions

let a = var as String # converts something to a string, raises an error if it cannot

# custom type conversions

interface To<Number> for TypeOrStructName
  fn to: Number
    # conversion of self to number logic here
    0
  end
end

interface From<Number> for TypeOrStructName
  fn from(val: Number): Self # called on the type: TypeOrStructName.from(5)
    # conversion from number to type here
    TypeOrStructName { a: 0 }
  end
end

# Templates

fn some_func<T>(a: T)
  # do something
end

fn some_func<T1, T2>(a: T1, b: T2): T1
  a # template types only exist for the analyzer, so `a as T2` is not allowed
end

interface Foo<T>
  fn a: T
end

interface From<T>
  fn from(val: T): Self
end

interface To<T>
  fn to: T
end

# range

let a = 1..10 # a range from 1 to 10
let a = 1...10 # a range from 1 to 9

# rest arguments: *args collects the remaining arguments into a table. there are no `...` varargs

fn foo(a, b, *args) # Lua: function foo(a, b, ...) local args = {...}
end

fn foo(a: any, b: any, callback: Function, *args: any) # *args is always last; `foo 1, 2 do ... end` puts the block in callback
end

# loops

while condition
end

something() while condition

until condition
end

something() until condition

loop
  # same as `while true`: runs until broken or returned away from
end

# counting
for i in 1..10
end

for i in 1..10 step 2
end

# same as `for _, i in ipairs(table) do`
# when table is not an iterator. this is decided from the declared type
# of table, or at runtime if it has none
for i in table
end

# same as `for _, i in pairs(table) do`
# under the same rule
for k, v in table
end

# general syntax
for vars in iterator
end

# calling methods on something

something.method # same syntax as function call

# unlike Lua, we do not differentiate between normal function call and method call
# self is always available on all struct and module functions

# functions that come from Lua get no self, as in Lua.
# a method written in Lua or C is called with a colon, and may have any capitalization
conn:sendMessage("hi")

# exporting stuff to be available from other files
# functions, structs, interfaces, enums, types, modules and constants can be exported

pub fn foo
end

# unlike rust, all struct members are public
# use local variables for private members
pub struct Foo
end

pub const limit = 10

# importing stuff from other NL files
# the imports themselves are at the top of this file: imports must be the
# first statements of a file, and inside a function or module they are an error
# a file runs once, however many files import it; later imports get the same exports
# to load a file while the program runs, use require or include (below)


# the file is looked for relative to the importing file, then relative to
# the working directory, then in the search path

add_path "mods/shared" # adds a directory to the search path

# Lua modules are loaded with require, as in Lua
let json = require("json")

# include loads a file by its path and runs it (loadfile, then a call)
include "scripts/setup.lua"

# Regex
# real regular expressions, backed by the Rust regex crate
# matching always runs in linear time, so there are no backreferences or lookarounds
# single-quoted strings keep backslashes as written, so patterns need no double escaping

let re = Regex.new '^(.+)uwu$'

re.test "hewwo uwu" # true
re.find "hewwo uwu" # 1, 9 (first and last position of the match, in characters)

let m = re.match "hewwo uwu" # nil if there is no match
let whole = m[0] # "hewwo uwu" (the whole match)
let group = m[1] # "hewwo " (the first capture group)

# named groups are stored under their names
let date = Regex.new '(?<year>\d{4})-(?<month>\d{2})'
let {year, month} = date.match "2026-10"

# iterate over every match
for m in date.scan text
end

date.replace text, '$month/$year' # replaces the first match
date.replace_all text, '$month/$year' # replaces every match
date.replace_all text do |m| # the replacement can be computed
  "#{m.month}/#{m.year}"
end

let parts = Regex.new(',\s*').split "a, b,c" # {"a", "b", "c"}

# a regex in a `when` matches if the value matches it
case message
when re
  # ...
end

# meta methods
# self is the left-hand value, or the value itself for unary operators.
# the left-hand value decides which function runs:
# `foo * 2` runs Foo.*, and `2 * foo` runs Number.*

fn Foo.+(right)
end

fn Foo.-(right)
end

# without an argument, it's negation
fn Foo.-
end

fn Foo.*(right)
end

fn Foo./(right)
end

fn Foo.%(right)
end

fn Foo.**(right)
end

fn Foo.//(right)
end

fn Foo.|(right)
end

fn Foo.&(right)
end

fn Foo.~
end

fn Foo.^(right)
end

fn Foo.>>(right)
end

fn Foo.<<(right)
end

fn Foo.==(right)
end

fn Foo.<(right)
end

fn Foo.<=(right)
end

# > and >= are optional. without them, `a > b` is worked out as `b < a`
fn Foo.>(right)
end

fn Foo.>=(right)
end

# asked when a Foo is on the right of a number, string or boolean, as in `2 + foo`.
# it returns the two values to use instead, and the operation is tried again with them
fn Foo.coerce(left)
  return Foo { a: left, b: "x" }, self
end

fn Foo.<>(right)
end

# what .len returns
fn Foo.len
end

# called for keys the struct instance does not have
fn Foo.[](key)
end

fn Foo.[]=(key, value)
end

fn Foo.()(*args)
end

# pattern matching

case val
when [123, 456]
  # will match if the table consists only of two elements
  # and their values are 123 and 456
when {1: 123, 2: 456}
  # will match if the first two elements of a table are
  # 123 and 456 respectively, the table may contain
  # other keys/values
when {role: "user"}
  # would trigger if table key "role" equals to "user"
  # the table may contain other keys/values
when [x, 456]
  # square brackets always go by position: matches a table of two
  # elements whose second one is 456, and puts the first one into x
else
  # ...
end

# destructuring

let {a, b} = tbl
let {a: Number, b: String} = tbl # will error if types mismatch
let {key: value} = tbl # extract value of "key" from tbl into a variable "value"
let {key: value: Number} = tbl # will error if type isn't number
let [a, b] = tbl # by position: a is tbl[1], b is tbl[2]

fn foo({a, b})
end

fn foo({a, b} as tbl)
end

fn foo({a: Number, b: String}: Table)
end

fn foo({a: Number, b: String} as tbl: Table)
end

fn foo ({key: value})
end

fn foo ({key: value: Number})
end

# type annotations aren't necessary if working with a struct, since
# struct types are already known

struct Foo
  a: Number
  b: String
end

fn foo({a: var1, b: var2}: Foo)
end

# leave variable name blank to destructure with the same
# names as the structure members

fn foo({a, b}: Foo)
end

# if let
# runs the branch only if the pattern matches, and binds its variables there

if let player = find_player(name)
  # player is not nil here
end

if let {role: "admin", name} = user
  # user.role is "admin", and name holds user.name
else
  # no match
end

# primitive operations

fn Number.is_cool
  self == 7
end

print 6.is_cool # false
print 7.is_cool # true

fn Number.divisible_by(n)
  self % n == 0
end

print 100.divisible_by(7) # false
print 100.divisible_by(10) # true

# methods also work on literals and on the built-in string and number functions

print "hello".upcase # HELLO

# exceptions

fn foo
  raise "error here"
end

try
  foo
catch err
  print "error was: #{err.message}"
end

struct MyException : Exception
  reason: String
end

fn foo
  raise MyException { reason: "i am not a clever fox" } # exceptions are built like any struct
end

try
  foo
catch err: MyException
  print "the error happened because #{err.reason}"
catch err: Exception
  # everything that can be caught is an Exception, errors raised by Lua included
  print "the error was #{err.message}"
finally
  # always runs, whether or not an error occurred
end

# enums

enum Direction # enums and their variants must follow PascalCase
  North
  East
  South
  West
end

enum Team
  Red = 1 # a variant may be given an explicit number or string value
  Blue = 2
end

let a = Direction.North

case a
when Direction.North
  # ...
when Direction.East, Direction.West
  # ...
end # the analyzer warns that South is not handled

# records
# a record is a type that describes the shape of a plain table

type Point = {x: Number, y: Number}

let a: Point = {x: 1, y: 2}

fn length(p: Point): Number
  (p.x ** 2 + p.y ** 2) ** 0.5
end

# default parameters

fn greet(name, greeting = "hello")
  print "#{greeting}, #{name}"
end

fn summon(kind: String, level: Number = 1)
end

# async

# calling an async function starts it and returns a Task right away
async fn load_profile(id)
  let res = http_get("/profile/#{id}").await # waits without blocking the game
  res.body
end

let task = load_profile(1)

task.on_done do |profile|
  # ...
end

# wrapping a callback-based API into a Task
fn http_get(url)
  Task.new do |resolve, reject|
    :http.request(url, resolve, reject)
  end
end

# event handlers can be async too
on PlayerJoined async do |player|
  Task.sleep(5).await
  # ...
end

# workers
# spawn runs a call or a block on another core, and gives back a Task.
# nothing is shared with a worker: what goes in and what comes out is copied

let task = spawn find_path(from, to)
let path = spawn find_path(from, to).await # start it and wait for it

let mesh = spawn do
  simplify(build_mesh(chunk), detail) # chunk and detail are copies in here
end

# a dedicated worker is a file that keeps running, with variables of its own
const nav = Worker.new 'nav_service'

let path = nav.find_path(from, to).await # calls a `pub fn` of that file
nav.emit "MapEdited", changes # runs the event's handlers in the worker
nav.stop
