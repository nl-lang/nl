/*
** The transfer encoder (LuaJIT/src/lj_nltransfer.c) between two states of
** one thread, with the real compiler and runtime: every kind of value that
** crosses, shared references and cycles, deep nesting and long lists, NaN and -0,
** exceptions with their traces, registered metatables, host userdata with
** nl_settransfer, and the SendError messages. Values are made in state A,
** encoded there, decoded in state B (which loads the files the message
** names) and checked by NL code in B.
**
** The encoder is internal to the VM; the test calls its two functions
** directly (they are in libluajit.a).
**
**   test_transfer DIR           the tests, in the scratch directory DIR
**   test_transfer DIR --bench   also the measurements:
**                               the encoder against string.buffer
*/

#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <time.h>
#include <math.h>
#include <unistd.h>

#include "lua.h"
#include "lauxlib.h"
#include "lualib.h"
#include "nl.h"

#include "support/check.h"

/* The C-heap checks need the system allocator's statistics: not under the
** sanitizers, whose allocators keep freed blocks in quarantine.
*/
#if defined(__has_feature)
#if __has_feature(address_sanitizer) || __has_feature(thread_sanitizer)
#define NL_SANITIZED 1
#endif
#endif
#if defined(__SANITIZE_ADDRESS__) || defined(__SANITIZE_THREAD__)
#define NL_SANITIZED 1
#endif

#if defined(__APPLE__) && !defined(NL_SANITIZED)
#include <malloc/malloc.h>
static long heap_in_use(void)
{
  malloc_statistics_t st;
  malloc_zone_statistics(NULL, &st);
  return (long)st.size_in_use;
}
#define HAVE_HEAP_IN_USE 1
#endif

/* lj_nltransfer.c (lj_nlint.h). */
typedef struct NLMsg NLMsg;
NLMsg *lj_nl_encode(lua_State *L, int base, int n, int labidx);
int lj_nl_decode(lua_State *L, const NLMsg *m);
void lj_nl_msgfree(NLMsg *m);
size_t lj_nl_msgsize(const NLMsg *m);

static void writefile(const char *name, const char *text)
{
  FILE *fp = fopen(name, "wb");
  if (!fp) { perror(name); exit(2); }
  fputs(text, fp);
  fclose(fp);
}

static const char *SHAPES =
  "# Values for test_transfer.c.\n"
  "pub struct Rect\n"
  "  w: Number\n"
  "  h: Number\n"
  "end\n"
  "\n"
  "fn Rect.area\n"
  "  @w * @h\n"
  "end\n"
  "\n"
  "pub struct Oops : Exception\n"
  "  code: Number\n"
  "end\n"
  "\n"
  "pub enum Color\n"
  "  Red\n"
  "  Blue\n"
  "end\n"
  "\n"
  "pub fn square(x)\n"
  "  x * x\n"
  "end\n"
  "\n"
  "pub const point_meta = {}\n"
  "Worker.register(\"Point\", point_meta)\n"
  "\n"
  "pub fn local_struct\n"
  "  struct Inner\n"
  "    a: Number\n"
  "  end\n"
  "  Inner { a: 1 }\n"
  "end\n";

/* -- Vec2: a host type with transfer functions -------------------------- */

typedef struct Vec2 { double x, y; } Vec2;

static int vec2_new(lua_State *L)
{
  Vec2 *v = (Vec2 *)lua_newuserdata(L, sizeof(Vec2));
  v->x = luaL_checknumber(L, 1);
  v->y = luaL_checknumber(L, 2);
  luaL_getmetatable(L, "Vec2");
  lua_setmetatable(L, -2);
  return 1;
}

static int vec2_sum(lua_State *L)
{
  Vec2 *v = (Vec2 *)luaL_checkudata(L, 1, "Vec2");
  lua_pushnumber(L, v->x + v->y);
  return 1;
}

static int vec2_encode(lua_State *L)
{
  Vec2 *v = (Vec2 *)luaL_checkudata(L, 1, "Vec2");
  lua_pushlstring(L, (const char *)v, sizeof(Vec2));
  return 1;
}

static int vec2_decode(lua_State *L)
{
  size_t len;
  const char *s = luaL_checklstring(L, 1, &len);
  Vec2 *v;
  if (len != sizeof(Vec2)) return luaL_error(L, "bad Vec2");
  v = (Vec2 *)lua_newuserdata(L, sizeof(Vec2));
  memcpy(v, s, sizeof(Vec2));
  luaL_getmetatable(L, "Vec2");
  lua_setmetatable(L, -2);
  return 1;
}

/* Host types whose transfer functions break the contract. */
static int bad_encode(lua_State *L) { return luaL_error(L, "the encode callback failed"); }
static int zero_encode(lua_State *L) { (void)L; return 0; }
static int num_encode(lua_State *L) { lua_pushnumber(L, 7); return 1; }
static int nil_decode(lua_State *L) { (void)L; return 0; }
static int id_decode(lua_State *L) { lua_settop(L, 1); return 1; }
static int raise_decode(lua_State *L) { return luaL_error(L, "the decode callback failed"); }
/* encode returns a table holding the userdata itself; or another host value. */
static int self_encode(lua_State *L) { lua_newtable(L); lua_pushvalue(L, 1); lua_rawseti(L, -2, 1); return 1; }
static int wrap_encode(lua_State *L) { lua_getglobal(L, "inner_vec"); return 1; }

static void reg_type(lua_State *L, const char *name, lua_CFunction enc, lua_CFunction dec)
{
  luaL_newmetatable(L, name);
  nl_settype(L, -1, name, 0);
  lua_pop(L, 1);
  lua_pushcfunction(L, enc);
  lua_pushcfunction(L, dec);
  nl_settransfer(L, name);
}

static int mk_typed(lua_State *L)
{
  const char *tn = luaL_checkstring(L, 1);
  lua_newuserdata(L, 8);
  luaL_getmetatable(L, tn);
  lua_setmetatable(L, -2);
  return 1;
}

/* A host type without transfer functions. */
static int thing_new(lua_State *L)
{
  lua_newuserdata(L, 4);
  luaL_getmetatable(L, "Thing");
  lua_setmetatable(L, -2);
  return 1;
}

static int openlibs(lua_State *L)
{
  luaL_openlibs(L);
  luaL_newmetatable(L, "Vec2");
  nl_settype(L, -1, "Vec2", 0);
  lua_pop(L, 1);
  luaL_newmetatable(L, "Thing");
  nl_settype(L, -1, "Thing", 0);
  lua_pop(L, 1);
  lua_pushcfunction(L, vec2_encode);
  lua_pushcfunction(L, vec2_decode);
  nl_settransfer(L, "Vec2");
  reg_type(L, "Bad", bad_encode, id_decode);
  reg_type(L, "Zero", zero_encode, id_decode);
  reg_type(L, "NilDec", num_encode, nil_decode);
  reg_type(L, "NumDec", num_encode, id_decode);
  reg_type(L, "RaiseDec", num_encode, raise_decode);
  reg_type(L, "Self", self_encode, id_decode);
  reg_type(L, "Wrap", wrap_encode, id_decode);
  lua_register(L, "mk", mk_typed);
  lua_register(L, "vec2", vec2_new);
  lua_register(L, "vec2_sum", vec2_sum);
  lua_register(L, "thing", thing_new);
  return 0;
}

static lua_State *newstate(void)
{
  lua_State *L = luaL_newstate();
  lua_pushcfunction(L, openlibs);
  if (lua_pcall(L, 0, 0, 0) != 0) {
    fprintf(stderr, "openlibs: %s\n", lua_tostring(L, -1));
    exit(1);
  }
  return L;
}

static const char *errtext(lua_State *L)
{
  if (lua_type(L, -1) == LUA_TSTRING) return lua_tostring(L, -1);
  return nlL_tolstring(L, -1, NULL);
}

/* Loads NL source and leaves its export table (or nil). */
static void donl(lua_State *L, const char *src, const char *name)
{
  int st = nl_loadbuffer(L, src, strlen(src), name);
  if (st == 0) st = nl_pcall(L, 0, 1);
  if (st != 0) {
    fprintf(stderr, "donl(%s): %s\n", name, errtext(L));
    lua_settop(L, 0);
    lua_pushnil(L);
    check_failed_total++; check_test_failed = 1;
  }
}

static int encode_f(lua_State *L)
{
  int n = lua_gettop(L) - 1;
  NLMsg *m = lj_nl_encode(L, 2, n, lua_istable(L, 1) ? 1 : 0);
  lua_pushlightuserdata(L, m);
  return 1;
}

static int decode_f(lua_State *L)
{
  NLMsg *m = (NLMsg *)lua_touserdata(L, 1);
  lua_remove(L, 1);
  return lj_nl_decode(L, m);
}

static char result[4096];

/* The n values on top of A, encoded with labels (a NULL-terminated list, or
** NULL), become a message; NULL with result set to "<Type>: <message>" when
** encoding fails. The values are popped.
*/
static NLMsg *encode(lua_State *A, int n, const char *const *labels)
{
  int base = lua_gettop(A) - n, i;
  NLMsg *m = NULL;
  lua_pushcfunction(A, encode_f);
  if (labels) {
    lua_newtable(A);
    for (i = 0; labels[i]; i++) {
      lua_pushstring(A, labels[i]);
      lua_rawseti(A, -2, i + 1);
    }
  } else {
    lua_pushnil(A);
  }
  for (i = 1; i <= n; i++) lua_pushvalue(A, base + i);
  if (nl_pcall(A, n + 1, 1) != 0) {
    const char *t = nl_typeof(A, -1);
    lua_getfield(A, -1, "message");
    snprintf(result, sizeof(result), "%s: %s", t ? t : "?",
	     lua_isstring(A, -1) ? lua_tostring(A, -1) : errtext(A));
    lua_settop(A, base);
    return NULL;
  }
  m = (NLMsg *)lua_touserdata(A, -1);
  lua_settop(A, base);
  return m;
}

/* Decodes m in B, pushing its values; returns their number, or -1 with
** result set.
*/
static int decode(lua_State *B, NLMsg *m)
{
  int top = lua_gettop(B);
  lua_pushcfunction(B, decode_f);
  lua_pushlightuserdata(B, m);
  if (nl_pcall(B, 1, LUA_MULTRET) != 0) {
    const char *t = nl_typeof(B, -1);
    lua_getfield(B, -1, "message");
    snprintf(result, sizeof(result), "decode %s: %s", t ? t : "?",
	     lua_isstring(B, -1) ? lua_tostring(B, -1) : errtext(B));
    lua_settop(B, top);
    return -1;
  }
  return lua_gettop(B) - top;
}

static lua_State *A, *B;

/* make() of make_src in A, sent to B, check(value) of check_src in B; the
** string check returns (or the error) is in result.
*/
static const char *trip(const char *make_src, const char *check_src,
			const char *const *labels)
{
  NLMsg *m;
  int n;
  lua_settop(A, 0);
  lua_settop(B, 0);
  donl(A, make_src, "=make");
  if (!lua_istable(A, -1)) return "no make";
  lua_getfield(A, -1, "make");
  if (nl_pcall(A, 0, 1) != 0) {
    snprintf(result, sizeof(result), "make failed: %s", errtext(A));
    return result;
  }
  m = encode(A, 1, labels);
  if (!m) return result;
  n = decode(B, m);
  lj_nl_msgfree(m);
  if (n < 0) return result;
  donl(B, check_src, "=check");
  if (!lua_istable(B, -1)) return "no check";
  lua_getfield(B, -1, "check");
  lua_pushvalue(B, 1);
  if (nl_pcall(B, 1, 1) != 0) {
    snprintf(result, sizeof(result), "check failed: %s", errtext(B));
    return result;
  }
  snprintf(result, sizeof(result), "%s", lua_isstring(B, -1) ? lua_tostring(B, -1) : "(not a string)");
  lua_settop(B, 0);
  return result;
}

#define IMPORT "import { Rect, Oops, Color, square, point_meta, local_struct } from 'shapes'\n"

static void test_scalars(void)
{
  CHECK_STR(trip(IMPORT "pub fn make\n  {a: true, b: false, n: 42, f: 2.5, s: \"text\", c: Color.Red, big: 1e300}\nend\n",
		 IMPORT "pub fn check(v)\n  \"#{v.a} #{v.b} #{v.n} #{v.f} #{v.s} #{v.c == Color.Red} #{v.big} #{v.none == nil}\"\nend\n", NULL),
	    "true false 42 2.5 text true 1e+300 true");
  /* nil alone, and a string with every byte. */
  CHECK_STR(trip(IMPORT "pub fn make\n  nil\nend\n",
		 IMPORT "pub fn check(v)\n  \"#{v == nil}\"\nend\n", NULL), "true");
  CHECK_STR(trip(IMPORT "pub fn make\n  let parts = []\n  for i in 0..255\n    parts[parts.len + 1] = :string.char(i)\n  end\n  :table.concat(parts)\nend\n",
		 IMPORT "pub fn check(v)\n  let ok = true\n  for i in 0..255\n    ok = false if :string.byte(v, i + 1) != i\n  end\n  \"#{:string.len(v)} #{ok}\"\nend\n", NULL),
	    "256 true");
}

static void test_numbers_bit_exact(void)
{
  NLMsg *m;
  int n;
  lua_settop(A, 0);
  lua_settop(B, 0);
  lua_pushnumber(A, 0.0 / 0.0);
  lua_pushnumber(A, -0.0);
  lua_pushnumber(A, 1.0 / 0.0);
  lua_pushnumber(A, -1.0 / 0.0);
  lua_pushnumber(A, 4.9e-324);
  lua_pushnumber(A, 0.1);
  lua_pushinteger(A, -2147483647 - 1);
  m = encode(A, 7, NULL);
  CHECK(m != NULL);
  if (!m) return;
  n = decode(B, m);
  lj_nl_msgfree(m);
  CHECK(n == 7);
  if (n != 7) return;
  CHECK(isnan(lua_tonumber(B, 1)));
  CHECK(lua_tonumber(B, 2) == 0 && signbit(lua_tonumber(B, 2)));
  CHECK(isinf(lua_tonumber(B, 3)) && lua_tonumber(B, 3) > 0);
  CHECK(isinf(lua_tonumber(B, 4)) && lua_tonumber(B, 4) < 0);
  CHECK(lua_tonumber(B, 5) == 4.9e-324);
  CHECK(lua_tonumber(B, 6) == 0.1);
  CHECK(lua_tonumber(B, 7) == -2147483648.0);
  lua_settop(B, 0);
}

static void test_tables(void)
{
  /* A deep copy of plain tables, records and lists, holes and all keys. */
  CHECK_STR(trip(IMPORT "pub fn make\n  let t = {list: [1, 2, 3], rec: {x: 1, y: {z: \"deep\"}}}\n  t[\"a b\"] = 5\n  t[0] = \"zero\"\n  t[2.5] = \"half\"\n  t[true] = \"yes\"\n  t.holes = [1, nil, 3]\n  t\nend\n",
		 IMPORT "pub fn check(v)\n  \"#{v.list.len} #{v.list[3]} #{v.rec.y.z} #{v[\"a b\"]} #{v[0]} #{v[2.5]} #{v[true]} #{v.holes[1]} #{v.holes[2] == nil} #{v.holes[3]}\"\nend\n", NULL),
	    "3 3 deep 5 zero half yes 1 true 3");
  /* A table as a key. */
  CHECK_STR(trip(IMPORT "pub fn make\n  let k = {name: \"key\"}\n  let t = {}\n  t[k] = k\n  t\nend\n",
		 IMPORT "pub fn check(v)\n  let found = \"none\"\n  for k, x in v\n    found = \"#{k.name} #{k == x}\"\n  end\n  found\nend\n", NULL),
	    "key true");
}

static void test_shape(void)
{
  /* A table referenced twice arrives as one table referenced twice; a
  ** cycle as a cycle.
  */
  CHECK_STR(trip(IMPORT "pub fn make\n  let shared = {n: 1}\n  let c = {}\n  c.again = c\n  {a: shared, b: shared, c: c, list: [shared, shared]}\nend\n",
		 IMPORT "pub fn check(v)\n  \"#{v.a == v.b} #{v.list[1] == v.a} #{v.c.again == v.c} #{v.c.again.again == v.c}\"\nend\n", NULL),
	    "true true true true");
  /* Between messages there is no identity; within one, across values too. */
  {
    NLMsg *m1, *m2;
    lua_settop(A, 0);
    lua_settop(B, 0);
    lua_newtable(A);
    lua_pushvalue(A, 1);
    m1 = encode(A, 1, NULL);
    lua_pushvalue(A, 1);
    m2 = encode(A, 1, NULL);
    lua_pushvalue(A, 1);
    lua_pushvalue(A, 1);
    {
      NLMsg *m3 = encode(A, 2, NULL);
      CHECK(m1 && m2 && m3);
      if (m1 && m2 && m3) {
	CHECK(decode(B, m1) == 1);
	CHECK(decode(B, m2) == 1);
	CHECK(!lua_rawequal(B, 1, 2));
	CHECK(decode(B, m3) == 2);
	CHECK(lua_rawequal(B, 3, 4));
      }
      if (m3) lj_nl_msgfree(m3);
    }
    if (m1) lj_nl_msgfree(m1);
    if (m2) lj_nl_msgfree(m2);
    lua_settop(B, 0);
  }
}

static void test_deep_and_long(void)
{
  /* Nesting far deeper than Lua's stack or the C stack would allow a
  ** recursive copy, and a long list.
  */
  CHECK_STR(trip(IMPORT "pub fn make\n  let root = {}\n  let t = root\n  for i in 1..200000\n    let n = {}\n    t.next = n\n    t = n\n  end\n  t.last = true\n  root\nend\n",
		 IMPORT "pub fn check(v)\n  let depth = 0\n  let t = v\n  while t.next != nil\n    t = t.next\n    depth += 1\n  end\n  \"#{depth} #{t.last}\"\nend\n", NULL),
	    "200000 true");
  CHECK_STR(trip(IMPORT "pub fn make\n  let l = []\n  for i in 1..1000000\n    l[i] = i * 2\n  end\n  l\nend\n",
		 IMPORT "pub fn check(v)\n  let s = 0\n  for x in v\n    s += x\n  end\n  \"#{v.len} #{v[1]} #{v[1000000]} #{s}\"\nend\n", NULL),
	    "1000000 2 2000000 1000001000000");
  /* Deeply nested lists inside lists (the array run of each). */
  CHECK_STR(trip(IMPORT "pub fn make\n  let root = [0]\n  let t = root\n  for i in 1..100000\n    let n = [i]\n    t[2] = n\n    t = n\n  end\n  root\nend\n",
		 IMPORT "pub fn check(v)\n  let depth = 0\n  let t = v\n  while t.len == 2\n    t = t[2]\n    depth += 1\n  end\n  \"#{depth} #{t[1]}\"\nend\n", NULL),
	    "100000 100000");
}

static void test_structs(void)
{
  /* An instance of the same struct: methods, `is` and patterns work. */
  CHECK_STR(trip(IMPORT "pub fn make\n  Rect { w: 2, h: 3 }\nend\n",
		 IMPORT "pub fn check(v)\n  let p = case v\n    when Rect { w: w }\n      w\n    else\n      -1\n  end\n  \"#{typeof(v)} #{v is Rect} #{v.area} #{p}\"\nend\n", NULL),
	    "Rect true 6 2");
  /* Structs and enums as values, by name. */
  CHECK_STR(trip(IMPORT "pub fn make\n  {r: Rect, o: Oops}\nend\n",
		 IMPORT "pub fn check(v)\n  \"#{v.r == Rect} #{v.o == Oops}\"\nend\n", NULL),
	    "true true");
  /* A struct declared inside a function has no name. */
  CHECK_STR(trip(IMPORT "pub fn make\n  {x: local_struct()}\nend\n",
		 IMPORT "pub fn check(v)\n  \"sent\"\nend\n", NULL),
	    "SendError: cannot send an instance of struct Inner, which has no name another state can find (it is declared inside a function or in a nested block, or another declaration of its module has its name) (argument 1, at .x)");
}

static void test_ranges_and_exceptions(void)
{
  CHECK_STR(trip(IMPORT "pub fn make\n  {r: 1..4, x: 2...5}\nend\n",
		 IMPORT "pub fn check(v)\n  let s = 0\n  for i in v.r\n    s += i\n  end\n  let t = 0\n  for i in v.x\n    t += i\n  end\n  \"#{typeof(v.r)} #{s} #{t}\"\nend\n", NULL),
	    "Range 10 9");
  /* An exception keeps its type, its fields and its trace. */
  CHECK_STR(trip(IMPORT "pub fn make\n  try\n    raise Oops { message: \"bad\", code: 7 }\n  catch e: Oops\n    e\n  end\nend\n",
		 IMPORT "pub fn check(v)\n  \"#{typeof(v)} #{v.message} #{v.code} #{v is Exception} #{v.trace is String} #{:string.find(v.trace, \"make\", 1, true) != nil}\"\nend\n", NULL),
	    "Oops bad 7 true true true");
  CHECK_STR(trip(IMPORT "pub fn make\n  try\n    :error({detail: \"lua\"})\n  catch e\n    e\n  end\nend\n",
		 IMPORT "pub fn check(v)\n  \"#{typeof(v)} #{v.value.detail}\"\nend\n", NULL),
	    "LuaError lua");
}

static void test_functions(void)
{
  /* A named function is the receiver's own function of that name. */
  CHECK_STR(trip(IMPORT "pub fn make\n  {f: square!}\nend\n",
		 IMPORT "pub fn check(v)\n  \"#{v.f == square!} #{v.f(5)}\"\nend\n", NULL),
	    "true 25");
  /* Other functions, tasks and coroutines: SendError, with the path. */
  CHECK_STR(trip(IMPORT "pub fn make\n  let n = 1\n  let f = fn(x) x + n end\n  {callbacks: [f!]}\nend\n",
		 IMPORT "pub fn check(v)\n  \"sent\"\nend\n", NULL),
	    "SendError: cannot send a function without a name (argument 1, at .callbacks[1])");
  CHECK_STR(trip(IMPORT "pub fn make\n  {t: Task.new do |resolve, reject| resolve(1) end}\nend\n",
		 IMPORT "pub fn check(v)\n  \"sent\"\nend\n", NULL),
	    "SendError: cannot send a task (argument 1, at .t)");
  CHECK_STR(trip(IMPORT "pub fn make\n  let co: any = Coroutine.create(square!)\n  [co]\nend\n",
		 IMPORT "pub fn check(v)\n  \"sent\"\nend\n", NULL),
	    "SendError: cannot send a coroutine (argument 1, at [1])");
  /* A function as a key, a key that is not a name. */
  CHECK_STR(trip(IMPORT "pub fn make\n  let t = {}\n  let n = 2\n  t[fn() n end] = 1\n  t\nend\n",
		 IMPORT "pub fn check(v)\n  \"sent\"\nend\n", NULL),
	    "SendError: cannot send a function without a name (argument 1, at [Function key])");
  CHECK_STR(trip(IMPORT "pub fn make\n  let n = 2\n  let t = {}\n  t[\"a b\"] = {inner: [1, fn() n end]}\n  t\nend\n",
		 IMPORT "pub fn check(v)\n  \"sent\"\nend\n", NULL),
	    "SendError: cannot send a function without a name (argument 1, at [\"a b\"].inner[2])");
}

static void test_regex(void)
{
  CHECK_STR(trip(IMPORT "pub fn make\n  let r = Regex.new(\"a+b\")\n  {r: r, again: r}\nend\n",
		 IMPORT "pub fn check(v)\n  \"#{typeof(v.r)} #{v.r.pattern} #{v.r.test(\"xaab\")} #{v.r.test(\"b\")} #{v.r == v.again} #{v.r == Regex.new(\"a+b\")}\"\nend\n", NULL),
	    "Regex a+b true false true true");
}

static void test_metatables_and_userdata(void)
{
  /* A metatable registered with Worker.register crosses by name. */
  CHECK_STR(trip(IMPORT "pub fn make\n  :setmetatable({x: 3}, point_meta)\nend\n",
		 IMPORT "pub fn check(v)\n  \"#{getmetatable(v) == point_meta} #{v.x}\"\nend\n", NULL),
	    "true 3");
  CHECK_STR(trip(IMPORT "pub fn make\n  {p: :setmetatable({}, {})}\nend\n",
		 IMPORT "pub fn check(v)\n  \"sent\"\nend\n", NULL),
	    "SendError: cannot send a table with a metatable that is not a struct (argument 1, at .p)");
  /* Host userdata: through its transfer functions; one userdata
  ** referenced twice arrives as one.
  */
  CHECK_STR(trip(IMPORT "pub fn make\n  let v = :vec2(1.5, 2)\n  {a: v, b: v}\nend\n",
		 IMPORT "pub fn check(v)\n  \"#{typeof(v.a)} #{:vec2_sum(v.a)} #{v.a == v.b}\"\nend\n", NULL),
	    "Vec2 3.5 true");
  CHECK_STR(trip(IMPORT "pub fn make\n  {list: [1, :thing()]}\nend\n",
		 IMPORT "pub fn check(v)\n  \"sent\"\nend\n", NULL),
	    "SendError: cannot send a value of type Thing (argument 1, at .list[2])");
}

static void test_contract(void)
{
  /* A table with the metatable of a host type is no host value: the
  ** stand-in's message (and Regex's metatable is one).
  */
  CHECK_STR(trip(IMPORT "pub fn make\n  {p: :setmetatable({}, :getmetatable(:vec2(1, 2)))}\nend\n",
		 IMPORT "pub fn check(v)\n  \"sent\"\nend\n", NULL),
	    "SendError: cannot send a table with a metatable that is not a struct (argument 1, at .p)");
  CHECK_STR(trip(IMPORT "pub fn make\n  [:vec2(1, 2), [1, :setmetatable({}, :getmetatable(:vec2(1, 2)))]]\nend\n",
		 IMPORT "pub fn check(v)\n  \"sent\"\nend\n", NULL),
	    "SendError: cannot send a table with a metatable that is not a struct (argument 1, at [2][2])");
  CHECK_STR(trip(IMPORT "pub fn make\n  {r: :setmetatable({}, Regex)}\nend\n",
		 IMPORT "pub fn check(v)\n  \"sent\"\nend\n", NULL),
	    "SendError: cannot send a table with a metatable that is not a struct (argument 1, at .r)");
  /* Transfer functions that raise or return nothing: SendError with the
  ** part and the path.
  */
  CHECK_STR(trip(IMPORT "pub fn make\n  {list: [1, :mk(\"Bad\")]}\nend\n",
		 IMPORT "pub fn check(v)\n  \"sent\"\nend\n", NULL),
	    "SendError: cannot send a value of type Bad: its encode function failed: the encode callback failed (argument 1, at .list[2])");
  CHECK_STR(trip(IMPORT "pub fn make\n  {a: :mk(\"Zero\")}\nend\n",
		 IMPORT "pub fn check(v)\n  \"sent\"\nend\n", NULL),
	    "SendError: cannot send a value of type Zero: its encode function returned nothing (argument 1, at .a)");
  CHECK_STR(trip(IMPORT "pub fn make\n  {a: :mk(\"NilDec\")}\nend\n",
		 IMPORT "pub fn check(v)\n  \"sent\"\nend\n", NULL),
	    "decode SendError: cannot receive a value of type NilDec: its decode function returned nil, not a userdata");
  CHECK_STR(trip(IMPORT "pub fn make\n  {a: :mk(\"NumDec\")}\nend\n",
		 IMPORT "pub fn check(v)\n  \"sent\"\nend\n", NULL),
	    "decode SendError: cannot receive a value of type NumDec: its decode function returned a Number, not a userdata");
  CHECK_STR(trip(IMPORT "pub fn make\n  {a: :mk(\"RaiseDec\")}\nend\n",
		 IMPORT "pub fn check(v)\n  \"sent\"\nend\n", NULL),
	    "decode SendError: cannot receive a value of type RaiseDec: its decode function failed: the decode callback failed");
}

static void test_host_nesting(void)
{
  NLMsg *m;
  /* A userdata inside the value its own encode function made of it. */
  lua_settop(A, 0);
  luaL_dostring(A, "return { list = { mk('Self') } }");
  m = encode(A, 1, NULL);
  CHECK(m == NULL);
  CHECK_STR(result, "SendError: cannot send a value of type Self inside the value its transfer function made of it (argument 1, at .list[1][1])");
  /* A host value whose payload is another host value sent in the message:
  ** they arrive as one userdata.
  */
  lua_settop(A, 0);
  lua_settop(B, 0);
  luaL_dostring(A, "inner_vec = vec2(3, 4) return { w = mk('Wrap'), v = inner_vec }");
  m = encode(A, 1, NULL);
  CHECK(m != NULL);
  if (m) {
    CHECK(decode(B, m) == 1);
    lj_nl_msgfree(m);
    lua_getfield(B, 1, "w");
    lua_getfield(B, 1, "v");
    CHECK(lua_rawequal(B, -1, -2));
    CHECK(lua_type(B, -1) == LUA_TUSERDATA);
  }
  lua_settop(A, 0);
  lua_settop(B, 0);
}

static void test_strings_once(void)
{
  /* A long string sent many times is written once. */
  NLMsg *m;
  int n;
  lua_settop(A, 0);
  lua_settop(B, 0);
  luaL_dostring(A, "local s = string.rep('x', 1048576) local l = {} for i = 1, 1000 do l[i] = s end return l");
  m = encode(A, 1, NULL);
  CHECK(m != NULL);
  if (!m) return;
  CHECK(lj_nl_msgsize(m) < (2u << 20));
  n = decode(B, m);
  lj_nl_msgfree(m);
  CHECK(n == 1);
  luaL_loadstring(B, "local l = ... local ok = #l == 1000 for i = 1, #l do ok = ok and #l[i] == 1048576 end return ok and l[1] == l[1000]");
  lua_insert(B, 1);
  lua_call(B, 1, 1);
  CHECK(lua_toboolean(B, -1));
  lua_settop(B, 0);
  lua_settop(A, 0);
}

static int enc_lua(lua_State *L)
{
  NLMsg *m = lj_nl_encode(L, 1, lua_gettop(L), 0);
  lj_nl_msgfree(m);
  return 0;
}

static void test_failed_encode_frees(void)
{
  /* A SendError at the end of a long list frees what the encoder had
  ** built at once, not at some later collection.
  */
#ifdef HAVE_HEAP_IN_USE
  long before, peak = 0;
  int i;
  lua_settop(A, 0);
  lua_register(A, "enc_lua", enc_lua);
  luaL_dostring(A, "big = {} for i = 1, 200000 do big[i] = i end big[#big + 1] = function() end");
  lua_gc(A, LUA_GCCOLLECT, 0);
  before = heap_in_use();
  for (i = 0; i < 40; i++) {
    long u;
    luaL_dostring(A, "return pcall(enc_lua, big)");
    CHECK(!lua_toboolean(A, -2));
    lua_settop(A, 0);
    u = heap_in_use() - before;
    if (u > peak) peak = u;
  }
  printf("# 40 failed encodes of a list of 200000: C heap at most %+ld KB\n", peak / 1024);
  CHECK(peak < (8 << 20));
#endif
}

/* Allocation failures injected into encode and decode: every one leaves
** both states usable, and nothing is left behind.
*/
static lua_Alloc fail_of;
static void *fail_oud;
static long fail_armed = -1, fail_hits;
static void *fail_alloc(void *ud, void *ptr, size_t osize, size_t nsize)
{
  (void)ud;
  if (nsize > 0 && nsize > (ptr ? osize : 0) && fail_armed >= 0) {
    if (fail_armed == 0) { fail_hits++; return NULL; }
    fail_armed--;
  }
  return fail_of(fail_oud, ptr, osize, nsize);
}

static long fail_k;
static int fenc_f(lua_State *L)
{
  NLMsg *m;
  fail_armed = fail_k;
  m = lj_nl_encode(L, 1, 1, 0);
  fail_armed = -1;
  lua_pushlightuserdata(L, m);
  return 1;
}

static int fdec_f(lua_State *L)
{
  NLMsg *m = (NLMsg *)lua_touserdata(L, 1);
  int n;
  lua_remove(L, 1);
  fail_armed = fail_k;
  n = lj_nl_decode(L, m);
  fail_armed = -1;
  return n;
}

static void test_alloc_failures(void)
{
  static const char *MAKE = IMPORT
    "pub fn make\n"
    "  let shared = {k: \"s\"}\n"
    "  let deep = {}\n  let t = deep\n  for i in 1..300\n    let n = {}\n    t.next = n\n    t = n\n  end\n"
    "  let v = :vec2(1, 2)\n"
    "  {a: shared, b: [shared, v, v, Rect { w: 2, h: 3 }, Rect, :setmetatable({x: 1}, point_meta), 1..3],\n"
    "   r: Regex.new(\"a+\"), deep: deep, f: square!, long: :string.rep(\"y\", 100)}\n"
    "end\n";
  static const char *CHECKSRC = IMPORT "pub fn check(v)\n  \"#{v.b[4].area} #{:vec2_sum(v.b[2])} #{v.b[2] == v.b[3]} #{v.r.test(\"aa\")}\"\nend\n";
  long k, okenc = 0, failenc = 0, okdec = 0, faildec = 0;
  lua_Alloc fa, fb;
  void *ua, *ub;
#ifdef HAVE_HEAP_IN_USE
  long before = 0;
#endif
  CHECK_STR(trip(MAKE, CHECKSRC, NULL), "6 3 true true");
  fa = lua_getallocf(A, &ua);
  fb = lua_getallocf(B, &ub);
  fail_of = fa;
  fail_oud = ua;
  if (fa != fb || ua != ub) {
    /* Each state has an allocator of its own: inject into A only. */
    printf("# allocation failures injected into the encoder only (the states have their own allocators)\n");
  }
  lua_settop(A, 0);
  donl(A, MAKE, "=make");
  lua_getfield(A, -1, "make");
  lua_call(A, 0, 1);
  lua_replace(A, 1);
  lua_settop(A, 1);
  lua_setallocf(A, fail_alloc, NULL);
  for (k = 0; k < 4000; k += (k < 300 ? 1 : 7)) {
    NLMsg *m;
    if (k == 20) {
#ifdef HAVE_HEAP_IN_USE
      before = heap_in_use();
#endif
    }
    lua_settop(A, 1);
    fail_k = k;
    lua_pushcfunction(A, fenc_f);
    lua_pushvalue(A, 1);
    if (lua_pcall(A, 1, 1, 0) != 0) { fail_armed = -1; failenc++; continue; }
    m = (NLMsg *)lua_touserdata(A, -1);
    okenc++;
    lua_settop(B, 0);
    lua_pushcfunction(B, fdec_f);
    lua_pushlightuserdata(B, m);
    fail_k = -1;
    if (lua_pcall(B, 1, LUA_MULTRET, 0) != 0) faildec++; else okdec++;
    lj_nl_msgfree(m);
    lua_settop(B, 0);
    lua_gc(A, LUA_GCCOLLECT, 0);
    lua_gc(B, LUA_GCCOLLECT, 0);
  }
  lua_setallocf(A, fa, ua);
  /* And the decoder, with B's allocator. */
  lua_settop(A, 1);
  {
    NLMsg *m = encode(A, 1, NULL);
    CHECK(m != NULL);
    if (m) {
      fail_of = fb;
      fail_oud = ub;
      lua_setallocf(B, fail_alloc, NULL);
      for (k = 0; k < 4000; k += (k < 300 ? 1 : 7)) {
	lua_settop(B, 0);
	fail_k = k;
	lua_pushcfunction(B, fdec_f);
	lua_pushlightuserdata(B, m);
	if (lua_pcall(B, 1, LUA_MULTRET, 0) != 0) faildec++; else okdec++;
	fail_armed = -1;
	lua_settop(B, 0);
	lua_gc(B, LUA_GCCOLLECT, 0);
      }
      lua_setallocf(B, fb, ub);
      lj_nl_msgfree(m);
    }
  }
  printf("# injected allocation failures: encode ok %ld failed %ld, decode ok %ld failed %ld, %ld injected\n",
	 okenc, failenc, okdec, faildec, fail_hits);
  CHECK(failenc > 0 && faildec > 0 && okenc > 0 && okdec > 0);
#ifdef HAVE_HEAP_IN_USE
  {
    long grown;
    lua_gc(A, LUA_GCCOLLECT, 0);
    lua_gc(B, LUA_GCCOLLECT, 0);
    grown = heap_in_use() - before;
    printf("# C heap across them: %+ld KB\n", grown / 1024);
    CHECK(grown < (1 << 20));
  }
#endif
  /* Both states still work. */
  CHECK_STR(trip(MAKE, CHECKSRC, NULL), "6 3 true true");
}

static void test_type_values(void)
{
  /* Enums, modules and interfaces cross by name, as structs do. */
  writefile("tvals.nl", "pub enum Shade\n  Light\n  Dark\nend\npub module Tools\n  fn double(x)\n    x * 2\n  end\nend\n"
	    "pub interface Named\n  fn name\nend\n");
  CHECK_STR(trip("import { Shade, Tools, Named } from 'tvals'\npub fn make\n  {e: Shade, m: Tools, i: Named}\nend\n",
		 "import { Shade, Tools, Named } from 'tvals'\npub fn check(v)\n  \"#{v.e == Shade} #{v.e.Dark} #{v.m == Tools} #{v.m.double(4)} #{v.i == Named}\"\nend\n", NULL),
	    "true Dark true 8 true");
}

static void test_labels(void)
{
  /* The part of the message: arguments numbered from 1 as written, `self`,
  ** a captured local, the result, the exception.
  */
  static const char *const self_labels[] = { "self", "argument 1", NULL };
  static const char *const cap_labels[] = { "captured `chunk`", NULL };
  static const char *const res_labels[] = { "result", NULL };
  static const char *const exc_labels[] = { "exception", NULL };
  static const char *const make_closure =
    IMPORT "pub fn make\n  let n = 1\n  let f = fn(x) x + n end\n  {f: f!}\nend\n";
  static const char *const check = IMPORT "pub fn check(v)\n  \"sent\"\nend\n";
  CHECK_STR(trip(make_closure, check, self_labels),
	    "SendError: cannot send a function without a name (self, at .f)");
  CHECK_STR(trip(make_closure, check, cap_labels),
	    "SendError: cannot send a function without a name (captured `chunk`, at .f)");
  CHECK_STR(trip(make_closure, check, res_labels),
	    "SendError: cannot send a function without a name (result, at .f)");
  CHECK_STR(trip(make_closure, check, exc_labels),
	    "SendError: cannot send a function without a name (exception, at .f)");
  /* The second value of a message is argument 2. */
  {
    NLMsg *m;
    lua_settop(A, 0);
    lua_pushnumber(A, 1);
    lua_pushthread(A);
    m = encode(A, 2, NULL);
    CHECK(m == NULL);
    CHECK_STR(result, "SendError: cannot send a coroutine (argument 2)");
  }
}

static void test_receiver_lacks(void)
{
  /* B lacks the registration of a metatable that A has. */
  NLMsg *m;
  lua_settop(A, 0);
  lua_settop(B, 0);
  donl(A, "pub const meta = {}\nWorker.register(\"OnlyInA\", meta)\npub fn make\n  :setmetatable({}, meta)\nend\n", "=onlya");
  lua_getfield(A, -1, "make");
  CHECK(nl_pcall(A, 0, 1) == 0);
  m = encode(A, 1, NULL);
  CHECK(m != NULL);
  if (!m) return;
  CHECK(decode(B, m) < 0);
  CHECK_CONTAINS(result, "SendError");
  CHECK_CONTAINS(result, "OnlyInA");
  lj_nl_msgfree(m);
}

/* -- Measurements ---------------------------------------------------------- */

static double now(void)
{
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return (double)ts.tv_sec + (double)ts.tv_nsec * 1e-9;
}

/* Entities of four fields, one a nested table
** with a metatable. NL's encoder (the metatable registered by name) against
** string.buffer (the metatable in its dictionary), encode in A and decode in
** B, best of five.
*/
static void bench(void)
{
  static const int sizes[] = { 1, 100, 10000 };
  static const char *mk =
    "local n, meta = ...\n"
    "local list = {}\n"
    "for i = 1, n do\n"
    "  list[i] = { id = i, name = 'unit' .. i, hp = 100, pos = setmetatable({ x = i, y = -i }, meta) }\n"
    "end\n"
    "return list\n";
  size_t k;
  printf("# encoder against string.buffer (encode in A, decode in B), best of 5:\n");
  for (k = 0; k < sizeof(sizes) / sizeof(sizes[0]); k++) {
    int n = sizes[k], pass, reps = n == 1 ? 10000 : n == 100 ? 1000 : 10;
    double best_nl = 1e9, best_sb = 1e9, best_enc = 1e9;
    size_t nlbytes = 0, sbbytes = 0;
    lua_settop(A, 0);
    lua_settop(B, 0);
    /* The metatable: registered with Worker.register in both states. */
    luaL_dostring(A, "local m = {} ; Point_meta = m ; return m");
    luaL_dostring(B, "local m = {} ; Point_meta = m ; return m");
    lua_settop(A, 0);
    lua_settop(B, 0);
    donl(A, "Worker.register(\"BenchPoint\", :Point_meta)\n", "=reg");
    donl(B, "Worker.register(\"BenchPoint\", :Point_meta)\n", "=reg");
    lua_settop(A, 0);
    lua_settop(B, 0);
    luaL_loadstring(A, mk);
    lua_pushinteger(A, n);
    lua_getglobal(A, "Point_meta");
    lua_call(A, 2, 1);  /* 1: the list */
    for (pass = 0; pass < 5; pass++) {
      double t0 = now();
      int r;
      for (r = 0; r < reps; r++) {
	NLMsg *m;
	/* Unprotected calls, as for string.buffer below. */
	lua_pushcfunction(A, encode_f);
	lua_pushnil(A);
	lua_pushvalue(A, 1);
	lua_call(A, 2, 1);
	m = (NLMsg *)lua_touserdata(A, -1);
	lua_pop(A, 1);
	nlbytes = lj_nl_msgsize(m);
	lua_pushcfunction(B, decode_f);
	lua_pushlightuserdata(B, m);
	lua_call(B, 1, 1);
	lj_nl_msgfree(m);
	lua_settop(B, 0);
      }
      t0 = (now() - t0) / reps;
      if (t0 < best_nl) best_nl = t0;
    }
    for (pass = 0; pass < 5; pass++) {
      double t0 = now();
      int r;
      for (r = 0; r < reps; r++) {
	lua_pushcfunction(A, encode_f);
	lua_pushnil(A);
	lua_pushvalue(A, 1);
	lua_call(A, 2, 1);
	lj_nl_msgfree((NLMsg *)lua_touserdata(A, -1));
	lua_pop(A, 1);
      }
      t0 = (now() - t0) / reps;
      if (t0 < best_enc) best_enc = t0;
    }
    /* string.buffer, with the metatable in its dictionary. */
    luaL_dostring(A, "local buf = require('string.buffer'); local sb = buf.new({ metatable = { Point_meta } }); "
		  "SBenc = function(list) return sb:reset():encode(list):get() end");
    luaL_dostring(B, "local buf = require('string.buffer'); local sb = buf.new({ metatable = { Point_meta } }); "
		  "SBdec = function(s) return sb:set(s):decode() end");
    for (pass = 0; pass < 5; pass++) {
      double t0 = now();
      int r;
      for (r = 0; r < reps; r++) {
	size_t len;
	const char *s;
	lua_getglobal(A, "SBenc");
	lua_pushvalue(A, 1);
	lua_call(A, 1, 1);
	s = lua_tolstring(A, -1, &len);
	sbbytes = len;
	lua_getglobal(B, "SBdec");
	lua_pushlstring(B, s, len);
	lua_call(B, 1, 1);
	lua_settop(B, 0);
	lua_pop(A, 1);
      }
      t0 = (now() - t0) / reps;
      if (t0 < best_sb) best_sb = t0;
    }
    printf("#   %5d entities: NL %9.2f us (encode %9.2f; %7lu bytes), string.buffer %9.2f us (%7lu bytes), ratio %.2f\n",
	   n, best_nl * 1e6, best_enc * 1e6, (unsigned long)nlbytes, best_sb * 1e6, (unsigned long)sbbytes,
	   best_nl / best_sb);
  }
}

/* --profile: decodes 10000 entities in a loop, for a profiler. */
static void profile(void)
{
  NLMsg *m;
  int i;
  lua_settop(A, 0);
  luaL_dostring(A, "Point_meta = {}");
  luaL_dostring(B, "Point_meta = {}");
  donl(A, "Worker.register(\"BenchPoint\", :Point_meta)\n", "=reg");
  donl(B, "Worker.register(\"BenchPoint\", :Point_meta)\n", "=reg");
  lua_settop(A, 0);
  lua_settop(B, 0);
  luaL_dostring(A, "local list = {} for i = 1, 10000 do list[i] = { id = i, name = 'unit' .. i, hp = 100, "
		"pos = setmetatable({ x = i, y = -i }, Point_meta) } end return list");
  m = encode(A, 1, NULL);
  for (i = 0; i < 3000; i++) {
    decode(B, m);
    lua_settop(B, 0);
  }
  lj_nl_msgfree(m);
}

int main(int argc, char **argv)
{
  int dobench = argc > 2 && strcmp(argv[2], "--bench") == 0;
  int doprofile = argc > 2 && strcmp(argv[2], "--profile") == 0;
  if (argc > 1 && chdir(argv[1]) != 0) { perror(argv[1]); return 2; }
  NL_VERSION_SYM();
  writefile("shapes.nl", SHAPES);
  A = newstate();
  B = newstate();
  if (doprofile) { profile(); return 0; }
  RUN_TEST(test_scalars);
  RUN_TEST(test_numbers_bit_exact);
  RUN_TEST(test_tables);
  RUN_TEST(test_shape);
  RUN_TEST(test_deep_and_long);
  RUN_TEST(test_structs);
  RUN_TEST(test_ranges_and_exceptions);
  RUN_TEST(test_functions);
  RUN_TEST(test_regex);
  RUN_TEST(test_metatables_and_userdata);
  RUN_TEST(test_labels);
  RUN_TEST(test_receiver_lacks);
  RUN_TEST(test_contract);
  RUN_TEST(test_host_nesting);
  RUN_TEST(test_strings_once);
  RUN_TEST(test_failed_encode_frees);
  RUN_TEST(test_alloc_failures);
  RUN_TEST(test_type_values);
  if (dobench) bench();
  lua_close(A);
  lua_close(B);
  return check_summary("test_transfer");
}
