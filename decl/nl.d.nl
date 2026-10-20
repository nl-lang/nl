# NL's own standard library: the string and table libraries, tasks,
# regexes, events, workers and the prelude functions, for the analyzer. Built into the
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
## runtime/nl_80_string.lua says which ones work on any bytes.
declare module String = :String
  fn length(s: String): Number
  fn upcase(s: String): String
  fn downcase(s: String): String
  fn capitalize(s: String): String
  fn swapcase(s: String): String
  fn upcase_first(s: String): String
  fn downcase_first(s: String): String
  fn snake_case(s: String): String
  fn kebab_case(s: String): String
  fn camel_case(s: String): String
  fn pascal_case(s: String): String
  fn title_case(s: String): String
  fn parameterize(s: String, **opts: {sep: String?}): String
  fn fold(s: String): String
  fn casecmp(s: String, other: String): Number
  fn equal_ignoring_case(s: String, other: String): Bool
  fn reverse(s: String): String
  fn chars(s: String): [String]
  fn codepoints(s: String): [Number]
  fn as_bytes(s: String): [Number]
  fn byteslice(s: String, i: Number | Range, j: Number?): String
  fn ord(s: String): Number
  fn index(s: String, sub: String, init: Number?): Number?
  fn start_with(s: String, prefix: String): Bool
  fn end_with(s: String, suffix: String): Bool
  fn sub(s: String, pattern: String | Regex, rep: String | Function): String
  fn gsub(s: String, pattern: String | Regex, rep: String | Function): String
  fn match(s: String, re: Regex | String, init: Number?): Table?
  fn scan(s: String, re: Regex | String): Function
  fn split(s: String, sep: String | Regex | nil, **opts: {keep_empty: Bool?, limit: Number?}): [String]
  fn strip(s: String): String
  fn lstrip(s: String): String
  fn rstrip(s: String): String
  fn squish(s: String): String
  fn is_empty(s: String): Bool
  fn is_blank(s: String): Bool
  fn include(s: String, sub: String): Bool
  fn contains(s: String, sub: String): Bool
  ## Non-overlapping occurrences of a substring (not Ruby's character set).
  fn count(s: String, sub: String): Number
  fn remove(s: String, sub: String): String
  fn insert(s: String, i: Number, other: String): String
  fn delete_prefix(s: String, prefix: String): String
  fn delete_suffix(s: String, suffix: String): String
  fn ensure_prefix(s: String, prefix: String): String
  fn ensure_suffix(s: String, suffix: String): String
  fn chomp(s: String, suffix: String?): String
  fn chop(s: String): String
  ## The lines without their line endings.
  fn lines(s: String): [String]
  fn each_line(s: String, f: Function)
  fn each_char(s: String, f: Function)
  fn indent(s: String, n: Number, pad: String?): String
  fn dedent(s: String): String
  fn truncate(s: String, n: Number, **opts: {omission: String?, separator: String?}): String
  ## A greedy word wrap; `measure` gives the width of a text.
  fn wrap(s: String, width: Number, **opts: {measure: Function?}): [String]
  fn tr(s: String, from: String, to: String): String
  fn squeeze(s: String, set: String?): String
  ## `{name}` from keyed arguments or a table, `{1}` from positional ones.
  fn fmt(s: String, *args: any, **values: Table): String
  fn to_number(s: String, base: Number?): Number?
  fn scrub(s: String, replacement: String?): String
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
  fn split(self, s: String, **opts: {keep_empty: Bool?, limit: Number?}): [String]
end

## NL's table library (runtime/nl_84_table.lua): Lua's `table`
## functions, and a collection library. A function whose first
## parameter is a table is a method of a value typed `[T]`, `{[K]: V}`
## or `Table` (not a record): `xs.map(f)` is `Table.map(xs, f)`, resolved by
## the analyzer. Type parameters are not checked: `T` is `any`.
declare module Table = :Table
  # Lua's, copied from `table`.
  fn insert(t: Table, pos_or_value: any, value: any?)
  fn remove(t: Table, pos: Number?): any
  fn concat(t: Table, sep: String?, i: Number?, j: Number?): String
  fn sort(t: Table, comp: Function?)
  fn maxn(t: Table): Number
  fn getn(t: Table): Number
  # Lists. Callbacks get the value and its index.
  fn map<T, U>(list: [T], f: Function): [U]
  fn filter<T>(list: [T], f: Function): [T]
  fn select<T>(list: [T], f: Function): [T]
  fn reject<T>(list: [T], f: Function): [T]
  fn reduce<T, A>(list: [T], init: any, f: Function?): A
  fn fold<T, A>(list: [T], init: any, f: Function?): A
  fn each<T>(list: [T], f: Function): [T]
  fn each_with_index<T>(list: [T], f: Function): [T]
  fn find<T>(list: [T], f: Function): T?
  fn find_index<T>(list: [T], f_or_value: any): Number?
  fn index_of<T>(list: [T], value: any, from: Number?): Number?
  fn includes<T>(list: [T], value: any): Bool
  fn include<T>(list: [T], value: any): Bool
  fn any<T>(list: [T], f: Function?): Bool
  fn all<T>(list: [T], f: Function?): Bool
  fn none<T>(list: [T], f: Function?): Bool
  fn one<T>(list: [T], f: Function?): Bool
  fn count<T>(list: [T], f_or_value: any?): Number
  fn partition<T>(list: [T], f: Function): ([T], [T])
  fn group_by<T, K>(list: [T], f: Function): {[K]: [T]}
  fn tally<T>(list: [T]): {[T]: Number}
  fn sort_by<T>(list: [T], f: Function): [T]
  fn sorted<T>(list: [T], lt: Function?): [T]
  fn min<T>(list: [T], lt: Function?): T?
  fn max<T>(list: [T], lt: Function?): T?
  fn min_by<T>(list: [T], f: Function): T?
  fn max_by<T>(list: [T], f: Function): T?
  fn sum<T>(list: [T], f: Function?): Number
  fn uniq<T>(list: [T], by: Function?): [T]
  fn flatten(list: [any], depth: Number?): [any]
  fn flat_map<T, U>(list: [T], f: Function): [U]
  fn slice<T>(list: [T], from: any, to: Number?): [T]
  fn take<T>(list: [T], n: Number): [T]
  fn drop<T>(list: [T], n: Number): [T]
  fn take_while<T>(list: [T], f: Function): [T]
  fn drop_while<T>(list: [T], f: Function): [T]
  fn first<T>(list: [T], n: Number?): any
  fn last<T>(list: [T], n: Number?): any
  fn reverse<T>(list: [T]): [T]
  fn zip<T>(list: [T], *others: [any]): [[any]]
  fn each_slice<T>(list: [T], n: Number): [[T]]
  fn chunks<T>(list: [T], n: Number): [[T]]
  fn each_cons<T>(list: [T], n: Number): [[T]]
  fn join<T>(list: [T], sep: String?): String
  fn compact<T>(list: [T]): [T]
  fn push<T>(list: [T], *values: any): [T]
  fn pop<T>(list: [T]): T?
  fn shift<T>(list: [T]): T?
  fn unshift<T>(list: [T], *values: any): [T]
  fn append<T>(a: [T], b: [T]): [T]
  fn extend<T>(a: [T], b: [T]): [T]
  fn shuffle<T>(list: [T], rng: Function?): [T]
  fn sample<T>(list: [T], rng: Function?): T?
  # Maps. Callbacks get the key and the value.
  fn keys<K, V>(t: {[K]: V}): [K]
  fn values<K, V>(t: {[K]: V}): [V]
  fn entries<K, V>(t: {[K]: V}): [[any]]
  fn sorted_keys<K, V>(t: {[K]: V}, lt: Function?): [K]
  fn each_sorted<K, V>(t: {[K]: V}, f: Function): {[K]: V}
  fn each_kv<K, V>(t: {[K]: V}, f: Function): {[K]: V}
  fn map_values<K, V>(t: {[K]: V}, f: Function): {[K]: any}
  fn map_keys<K, V>(t: {[K]: V}, f: Function): {[any]: V}
  fn map_kv<K, V>(t: {[K]: V}, f: Function): Table
  fn filter_kv<K, V>(t: {[K]: V}, f: Function): {[K]: V}
  fn reject_kv<K, V>(t: {[K]: V}, f: Function): {[K]: V}
  fn invert<K, V>(t: {[K]: V}): {[V]: K}
  fn to_set<T>(list: [T]): {[T]: Bool}
  fn merge(a: Table, *more: Table): Table
  fn merge_into(a: Table, *more: Table): Table
  fn deep_merge(a: Table, b: Table): Table
  fn copy(t: Table): Table
  fn deep_copy(t: Table): Table
  fn equal(a: Table, b: Table): Bool
  fn deep_equal(a: Table, b: any): Bool
  fn is_empty(t: Table): Bool
  fn size(t: Table): Number
  fn has_key(t: Table, key: any): Bool
  fn get(t: Table, key: any, default: any?): any
  fn dig(t: Table, *keys: any): any
  fn pick(t: Table, keys: [any]): Table
  fn omit(t: Table, keys: [any]): Table
  fn clear(t: Table): Table
  # Making tables: not methods.
  fn range(first: Number, last: Number, step: Number?): [Number]
  fn fill(n: Number, value: any): [any]
  fn from_iter(f: Function, s: any?, c: any?): [any]
  fn pack(*values: any): Table
  fn unpack(t: Table, i: Number?, j: Number?): any
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
