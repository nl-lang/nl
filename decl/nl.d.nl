# NL's own standard library: the string library, tasks, regexes, events,
# workers and the prelude functions, for the analyzer. Built into the
# compiler; the names are prelude names, so the Lua paths here repeat
# what the prelude emits.
#
# The instance methods of `Regex` and `Task` take `self`; `Task.await`
# and the callbacks are typed by the analyzer itself, since `Task<T>`
# carries its result type (crates/nl_sema/src/analyzer/expr.rs).

declare fn typeof(value: any): String
declare fn raise(error: any): never
declare fn include(path: String): any
declare fn add_path(dir: String)
declare fn load_nl(source: String, name: String?): Function
declare fn keyed(t: Table): Table

## NL's string library: every function is a method of a
## string, `s.upcase`, with the string as its first argument.
declare module String = :String
  fn length(s: String): Number
  fn upcase(s: String): String
  fn downcase(s: String): String
  fn capitalize(s: String): String
  fn swapcase(s: String): String
  fn reverse(s: String): String
  fn chars(s: String): [String]
  fn codepoints(s: String): [Number]
  fn as_bytes(s: String): [Number]
  fn byteslice(s: String, i: Number, j: Number?): String
  fn ord(s: String): Number
  fn index(s: String, sub: String, init: Number?): Number?
  fn start_with(s: String, prefix: String): Bool
  fn end_with(s: String, suffix: String): Bool
  fn sub(s: String, pattern: String | Regex, rep: String | Function): String
  fn gsub(s: String, pattern: String | Regex, rep: String | Function): String
  fn match(s: String, re: Regex | String, init: Number?): Table?
  fn scan(s: String, re: Regex | String): Function
  fn split(s: String, sep: String | Regex | nil): [String]
  fn strip(s: String): String
  fn lstrip(s: String): String
  fn rstrip(s: String): String
  fn center(s: String, width: Number, pad: String?): String
  fn ljust(s: String, width: Number, pad: String?): String
  fn rjust(s: String, width: Number, pad: String?): String
  fn format(fmt: String, *args: any): String
  fn rep(s: String, n: Number, sep: String?): String
  fn valid_encoding(s: String): Bool
end

## Regular expressions. Positions count characters.
declare module Regex
  fn new(pattern: String): Regex
  pattern: String
  fn test(self, s: String): Bool
  fn find(self, s: String, init: Number?): (Number?, Number?)
  fn match(self, s: String, init: Number?): Table?
  fn scan(self, s: String): Function
  fn replace(self, s: String, rep: String | Function): String
  fn replace_all(self, s: String, rep: String | Function): String
  fn split(self, s: String): [String]
end

## Tasks. `task.await`, `on_done` and `on_error` are typed
## by the analyzer from `Task<T>`.
declare module Task
  fn new(executor: Function): Task
  fn all(tasks: [Task]): Task
  fn race(tasks: [Task]): Task
  fn sleep(seconds: Number): Task
end

## The event library.
declare module Event = :event
  fn run(name: String, *args: any): any
  fn on_run(name: String, handler: String, callback: Function)
  fn remove(name: String, handler: String)
end

## Workers. `Worker.new` with a literal path gives a typed
## handle (the analyzer reads the worker file); the handle type is not
## spelled in declarations, so it is `any` here.
declare module Worker = :worker
  fn new(path: String): any
  fn run(f: Function, *args: any): Task
  fn map(list: Table, f: Function, *args: any): Task
  fn register(name: String, metatable: Table)
  const parent: any
  const count: Number
end

## Lengths of time (runtime/nl_86_time.lua): an immutable number of
## seconds. `5.minutes` and the other Number helpers make one. The
## operators are the metamethods of its values.
declare module Duration
  fn new(seconds: Number): Duration
  fn seconds(n: Number): Duration
  fn minutes(n: Number): Duration
  fn hours(n: Number): Duration
  fn days(n: Number): Duration
  fn weeks(n: Number): Duration
  fn ms(n: Number): Duration
  fn total_seconds(self): Number
  fn total_ms(self): Number
  fn total_minutes(self): Number
  fn total_hours(self): Number
  fn total_days(self): Number
  ## The duration in whole days, hours, minutes, seconds and
  ## milliseconds, each with the duration's sign.
  fn parts(self): {days: Number, hours: Number, minutes: Number, seconds: Number, ms: Number}
  fn abs(self): Duration
  fn to_s(self): String
  fn humanize(self): String
  fn iso(self): String
  fn ago(self): Time
  fn from_now(self): Time
  fn +(right: Duration): Duration
  fn -(right: Duration): Duration
  fn *(right: Number): Duration
  ## By a Number, a Duration; by a Duration, their ratio (a Number).
  ## Either may come out, so the result is `any`.
  fn /(right: Number | Duration): any
  fn -: Duration
end

## Points in time (runtime/nl_86_time.lua): an immutable number of
## seconds since the Unix epoch, without a time zone. What reads the
## calendar takes `utc: true` for UTC, and is in local time otherwise.
declare module Time
  now: Time
  fn at(seconds: Number): Time
  fn new(year: Number, month: Number?, day: Number?, hour: Number?, min: Number?, sec: Number?, **opts: {utc: Bool?}): Time
  ## `YYYY-MM-DD`, with `THH:MM`, `:SS`, `.fraction`, and `Z` or
  ## `±HH:MM`; without a zone, local time (or UTC with `utc: true`).
  fn parse(s: String, **opts: {utc: Bool?}): Time
  fn epoch(self): Number
  fn year(self, **opts: {utc: Bool?}): Number
  fn month(self, **opts: {utc: Bool?}): Number
  fn day(self, **opts: {utc: Bool?}): Number
  fn hour(self, **opts: {utc: Bool?}): Number
  fn min(self, **opts: {utc: Bool?}): Number
  fn sec(self, **opts: {utc: Bool?}): Number
  ## 0 is Sunday.
  fn wday(self, **opts: {utc: Bool?}): Number
  fn yday(self, **opts: {utc: Bool?}): Number
  fn utc_offset(self): Duration
  fn strftime(self, format: String, **opts: {utc: Bool?}): String
  fn iso(self, **opts: {utc: Bool?}): String
  fn to_s(self): String
  fn date(self, **opts: {utc: Bool?}): String
  fn clock(self, **opts: {utc: Bool?}): String
  fn beginning_of_day(self, **opts: {utc: Bool?}): Time
  fn floor_to(self, step: Duration): Time
  fn since(self, earlier: Time): Duration
  fn until(self, later: Time): Duration
  fn humanize(self, now: Time?): String
  fn ago_in_words(self, now: Time?): String
  fn +(right: Duration): Time
  ## Minus a Duration, a Time; minus a Time, the Duration between
  ## (`since` is that, typed). Either may come out, so the result is `any`.
  fn -(right: Duration | Time): any
end
