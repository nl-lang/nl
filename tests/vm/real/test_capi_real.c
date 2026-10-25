/*
** The functions of nl.h against the real runtime (runtime/, the .lua files) and the
** real compiler (crates/nl_capi): every function that calls rt.* runs here
** with the real library. Built and run by tests/vm/real/run.sh, with a
** scratch directory as argv[1]; the test works inside it.
**
** The fake-runtime tests (tests/vm/test_capi.c) check the C side in
** detail; these check that the C side and the real runtime fit together.
*/

#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <sys/stat.h>
#include <unistd.h>

#include "lua.h"
#include "lauxlib.h"
#include "lualib.h"
#include "nl.h"

#include "support/check.h"

static int openlibs(lua_State *L)
{
  luaL_openlibs(L);
  return 0;
}

static int ndiag;
static char lastdiag[512];
static void reporter(void *data, const nl_Diagnostic *d)
{
  (void)data;
  ndiag++;
  snprintf(lastdiag, sizeof(lastdiag), "%s:%d:%d: [%s] %s", d->file, d->line,
	   d->column, d->code, d->message);
}

static lua_State *newstate(void)
{
  lua_State *L = luaL_newstate();
  nl_setreporter(L, reporter, NULL);
  lua_pushcfunction(L, openlibs);
  if (lua_pcall(L, 0, 0, 0) != 0) {
    fprintf(stderr, "luaL_openlibs: %s\n", lua_tostring(L, -1));
    exit(1);
  }
  return L;
}

static void writefile(const char *name, const char *text)
{
  FILE *fp = fopen(name, "wb");
  if (!fp) { perror(name); exit(2); }
  fputs(text, fp);
  fclose(fp);
}

static const char *errtext(lua_State *L)
{
  if (lua_type(L, -1) == LUA_TSTRING) return lua_tostring(L, -1);
  return nlL_tolstring(L, -1, NULL);
}

/* Runs NL source; leaves the export table. 0 on success. */
static int donl(lua_State *L, const char *src, const char *name)
{
  int st = nl_loadbuffer(L, src, strlen(src), name);
  if (st == 0) st = nl_pcall(L, 0, 1);
  if (st != 0) {
    fprintf(stderr, "donl(%s): %s\n", name, errtext(L));
    if (lua_type(L, -1) != LUA_TSTRING) lua_pop(L, 1);
    lua_pop(L, 1);
    lua_pushnil(L);
    check_failed_total++; check_test_failed = 1;
  }
  return st;
}

static int dolua(lua_State *L, const char *code)
{
  int top = lua_gettop(L);
  if (luaL_loadstring(L, code) || lua_pcall(L, 0, LUA_MULTRET, 0)) {
    fprintf(stderr, "dolua: %s\n", lua_tostring(L, -1));
    check_failed_total++; check_test_failed = 1;
    lua_settop(L, top);
    return 0;
  }
  return lua_gettop(L) - top;
}

#define CHECK_LUA(L, expr) do { \
  int n_ = dolua(L, "return " expr); \
  check_count++; \
  if (n_ < 1 || !lua_toboolean(L, -1)) { \
    check_failed_total++; check_test_failed = 1; \
    fprintf(stderr, "%s:%d: CHECK_LUA failed: %s\n", __FILE__, __LINE__, expr); \
  } \
  lua_pop(L, n_); \
} while (0)

/* Calls field `fn` of the table at idx with nargs arguments on top; 0 on
** success, results left; on failure the exception is left.
*/
static int callf(lua_State *L, int idx, const char *fn, int nargs, int nres)
{
  int st;
  idx = idx < 0 ? lua_gettop(L) + idx + 1 : idx;
  if (!lua_istable(L, idx)) {
    fprintf(stderr, "  %s(): no export table\n", fn);
    lua_pop(L, nargs);
    lua_pushliteral(L, "no export table");
    return -1;
  }
  lua_getfield(L, idx, fn);
  lua_insert(L, -(nargs+1));
  st = nl_pcall(L, nargs, nres);
  if (st) {
    int top = lua_gettop(L);
    fprintf(stderr, "  %s(): %s\n", fn, errtext(L));
    lua_settop(L, top);  /* errtext may push the text. */
  }
  return st;
}

/* -- Opening -------------------------------------------------------------- */

static void test_open(void)
{
  lua_State *L = newstate();
  CHECK_LUA(L, "type(typeof) == 'function' and type(include) == 'function'");
  CHECK_LUA(L, "type(String) == 'table' and type(load_nl) == 'function'");
  CHECK_LUA(L, "not pcall(string.dump, event.on_run)");  /* The C library. */
  lua_close(L);
}

/* An embedder that opens NL late, in an environment without loadstring
** (Garry's Mod's), with fields of its own in the host table: the runtime
** loads compiled chunks with the VM's host.loadstring, and leaves the
** prelude globals named in skip_globals alone.
*/
static int started;
static int late_start(lua_State *L)
{
  started = lua_istable(L, 1);
  return 0;
}

static int late_host(lua_State *L)
{
  lua_newtable(L);
  lua_pushboolean(L, 1);
  lua_setfield(L, -2, "typeof");
  lua_pushboolean(L, 1);
  lua_setfield(L, -2, "include");
  lua_setfield(L, 1, "skip_globals");
  lua_pushcfunction(L, late_start);
  return 1;
}

static void test_open_late(void)
{
  lua_State *L = luaL_newstate();
  nl_setreporter(L, reporter, NULL);
  CHECK(nl_setoption(L, NL_OPT_OPENLIBS, 0) == 1);
  lua_pushcfunction(L, openlibs);
  CHECK(lua_pcall(L, 0, 0, 0) == 0);
  CHECK_LUA(L, "typeof == nil and String == nil");
  dolua(L, "loadstring, load = nil, nil");
  started = 0;
  nl_sethostinit(L, late_host);
  lua_pushcfunction(L, luaopen_nl);
  CHECK(lua_pcall(L, 0, 0, 0) == 0);
  CHECK(started == 1);
  CHECK_LUA(L, "typeof == nil and type(String) == 'table'");
  CHECK(donl(L, "pub fn twice(x: Number): Number\n  x * 2\nend\n", "=late") == 0);
  lua_getfield(L, -1, "twice");
  lua_pushnumber(L, 21);
  CHECK(lua_pcall(L, 1, 1, 0) == 0);
  CHECK(lua_tonumber(L, -1) == 42);
  lua_close(L);
}

/* -- Types and values ----------------------------------------------------- */

static void test_builtin_types(void)
{
  lua_State *L = newstate();
  const char *names[] = { "Exception", "LuaError", "TypeError", "MatchError",
			  "ConversionError", "Range", "Task", NULL };
  int i;
  for (i = 0; names[i]; i++) {
    nl_gettype(L, names[i]);
    if (lua_isnil(L, -1)) fprintf(stderr, "  nl_gettype(\"%s\") is nil\n", names[i]);
    CHECK(!lua_isnil(L, -1));
    lua_pop(L, 1);
  }
  nl_gettype(L, "Regex");
  printf("# nl_gettype(\"Regex\") is %s\n",
	 lua_isnil(L, -1) ? "nil" : "a type");
  lua_pop(L, 1);
  nl_gettype(L, "NoSuchType");
  CHECK(lua_isnil(L, -1));
  lua_pop(L, 1);
  lua_pushnumber(L, 1.5); CHECK_STR(nl_typeof(L, -1), "Number"); lua_pop(L, 1);
  lua_pushliteral(L, "s"); CHECK_STR(nl_typeof(L, -1), "String"); lua_pop(L, 1);
  lua_pushboolean(L, 1); CHECK_STR(nl_typeof(L, -1), "Bool"); lua_pop(L, 1);
  lua_newtable(L); CHECK_STR(nl_typeof(L, -1), "Table"); lua_pop(L, 1);
  lua_pushcfunction(L, openlibs); CHECK_STR(nl_typeof(L, -1), "Function"); lua_pop(L, 1);
  lua_close(L);
}

static int settype_twice(lua_State *L)
{
  lua_newtable(L);
  nl_settype(L, -1, "Unit", 0);
  lua_newtable(L);
  nl_settype(L, -1, "Unit", 0);
  return 0;
}

static int check_unit(lua_State *L)
{
  nl_gettype(L, "Unit");
  nlL_checkisa(L, 1, -1);
  lua_pushliteral(L, "accepted");
  return 1;
}

static int typeerror_fn(lua_State *L)
{
  return nlL_typeerror(L, 1, "Widget");
}

static void test_host_types(void)
{
  lua_State *L = newstate();
  luaL_newmetatable(L, "Unit");
  nl_settype(L, -1, "Unit", 0);
  luaL_newmetatable(L, "Hero");
  nl_settype(L, -1, "Hero", -2);
  lua_settop(L, 0);
  lua_newuserdata(L, 8); luaL_getmetatable(L, "Hero"); lua_setmetatable(L, -2);  /* 1 */
  lua_newuserdata(L, 8); luaL_getmetatable(L, "Unit"); lua_setmetatable(L, -2);  /* 2 */
  CHECK_STR(nl_typeof(L, 1), "Hero");
  CHECK_STR(nl_typeof(L, 2), "Unit");
  nl_gettype(L, "Unit");  /* 3 */
  nl_gettype(L, "Hero");  /* 4 */
  CHECK(!lua_isnil(L, 3) && !lua_isnil(L, 4));
  CHECK(nl_isa(L, 1, 3) && nl_isa(L, 1, 4));
  CHECK(nl_isa(L, 2, 3) && !nl_isa(L, 2, 4));
  /* nl_typename names a type; a value of the type is not one. */
  CHECK_STR(nl_typename(L, 3), "Unit");
  CHECK_STR(nl_typename(L, 4), "Hero");
  CHECK(nl_typename(L, 1) == NULL);
  lua_settop(L, 2);
  /* NL code sees the host type's name through typeof. (An annotation or
  ** `is` naming it needs a declaration file: the compiler rejects an
  ** undeclared type name.)
  */
  CHECK(donl(L, "pub fn type_name(u)\n  typeof(u)\nend\n", "=hosttypes") == 0);
  lua_pushvalue(L, 2);
  CHECK(callf(L, -2, "type_name", 1, 1) == 0);
  CHECK_STR(lua_tostring(L, -1), "Unit");
  lua_settop(L, 2);
  /* A second registration of the name raises. */
  lua_pushcfunction(L, settype_twice);
  CHECK(nl_pcall(L, 0, 0) != 0);
  lua_pop(L, 1);
  /* nlL_checkisa and nlL_typeerror. */
  lua_pushcfunction(L, check_unit);
  lua_pushvalue(L, 1);
  CHECK(nl_pcall(L, 1, 1) == 0);
  CHECK_STR(lua_tostring(L, -1), "accepted");
  lua_pop(L, 1);
  lua_pushcfunction(L, check_unit);
  lua_pushnumber(L, 1);
  CHECK(nl_pcall(L, 1, 1) != 0);
  CHECK_STR(nl_typeof(L, -1), "TypeError");
  lua_pop(L, 1);
  lua_pushcfunction(L, typeerror_fn);
  lua_pushboolean(L, 0);
  CHECK(nl_pcall(L, 1, 0) != 0);
  CHECK_STR(nl_typeof(L, -1), "TypeError");
  lua_getfield(L, -1, "message");
  CHECK_CONTAINS(lua_tostring(L, -1), "Widget");
  lua_close(L);
}

static void test_structs_and_new(void)
{
  lua_State *L = newstate();
  int exp;
  CHECK(donl(L, "pub struct Point\n  x: Number\n  y: Number = 2\nend\n", "=structs") == 0);
  exp = lua_gettop(L);
  lua_getfield(L, exp, "Point");  /* exp+1 */
  CHECK(!lua_isnil(L, -1));
  lua_newtable(L);
  lua_pushnumber(L, 1);
  lua_setfield(L, -2, "x");
  nl_new(L, exp + 1);
  CHECK(lua_gettop(L) == exp + 2);
  CHECK_STR(nl_typeof(L, -1), "Point");
  CHECK(nl_isa(L, -1, exp + 1));
  lua_getfield(L, -1, "y");
  CHECK(lua_tonumber(L, -1) == 2);
  lua_pop(L, 2);
  lua_settop(L, 0);
  /* nl_typename: the kinds of type NL declares, built-in types, and NULL
  ** for what is not a type (a module, an instance, a plain table, a number).
  */
  CHECK(donl(L, "pub struct Point\n  x: Number\nend\npub enum Color\n  Red\nend\n"
	    "pub interface Named\n  fn title: String\nend\n"
	    "pub module Tools\n  fn hammer\n    1\n  end\nend\n", "=typenames") == 0);
  exp = lua_gettop(L);
  lua_getfield(L, exp, "Point"); CHECK_STR(nl_typename(L, -1), "Point"); lua_pop(L, 1);
  lua_getfield(L, exp, "Color"); CHECK_STR(nl_typename(L, -1), "Color"); lua_pop(L, 1);
  lua_getfield(L, exp, "Named"); CHECK_STR(nl_typename(L, -1), "Named"); lua_pop(L, 1);
  lua_getfield(L, exp, "Tools"); CHECK(nl_typename(L, -1) == NULL); lua_pop(L, 1);
  lua_getfield(L, exp, "Point");
  lua_newtable(L);
  lua_pushnumber(L, 2);
  lua_setfield(L, -2, "x");
  nl_new(L, -2);
  CHECK(nl_typename(L, -1) == NULL);
  CHECK_STR(nl_typename(L, -2), nl_typeof(L, -1));
  lua_pop(L, 2);
  nl_gettype(L, "Task"); CHECK_STR(nl_typename(L, -1), "Task"); lua_pop(L, 1);
  nl_gettype(L, "TypeError"); CHECK_STR(nl_typename(L, -1), "TypeError"); lua_pop(L, 1);
  CHECK(nl_typename(L, exp) == NULL);
  lua_pushnumber(L, 1); CHECK(nl_typename(L, -1) == NULL); lua_pop(L, 1);
  lua_settop(L, 0);
  lua_close(L);
}

/* Keyed arguments from C: nl_keyed marks the table that
** a `**opts` parameter takes; a plain table is a positional argument.
*/
static void test_keyed(void)
{
  lua_State *L = newstate();
  int exp;
  CHECK(donl(L, "pub fn takes(a, **opts)\n  \"#{a} #{opts.k} #{typeof(opts)}\"\nend\n"
	     "pub fn plain(a, b)\n  \"#{a} #{typeof(b)}\"\nend\n", "=keyed") == 0);
  exp = lua_gettop(L);
  lua_getfield(L, exp, "takes");
  lua_pushnumber(L, 1);
  lua_newtable(L);
  lua_pushnumber(L, 5);
  lua_setfield(L, -2, "k");
  nl_keyed(L, -1);
  CHECK(lua_pcall(L, 2, 1, 0) == 0);
  CHECK_STR(lua_tostring(L, -1), "1 5 Table");
  lua_pop(L, 1);
  /* Without nl_keyed the table is an ordinary argument. */
  lua_getfield(L, exp, "plain");
  lua_pushnumber(L, 1);
  lua_newtable(L);
  CHECK(lua_pcall(L, 2, 1, 0) == 0);
  CHECK_STR(lua_tostring(L, -1), "1 Table");
  lua_settop(L, 0);
  lua_close(L);
}

/* A field of the wrong type: nl_new raises TypeError. */
static int new_with_string_x(lua_State *L)
{
  lua_getglobal(L, "Q");
  lua_newtable(L);
  lua_pushliteral(L, "not a number");
  lua_setfield(L, -2, "x");
  nl_new(L, -2);
  return 1;
}

static void test_new_checks_fields(void)
{
  lua_State *L = newstate();
  CHECK(donl(L, "pub struct Q\n  x: Number\nend\n", "=q") == 0);
  lua_getfield(L, -1, "Q");
  lua_setglobal(L, "Q");
  lua_pop(L, 1);
  lua_pushcfunction(L, new_with_string_x);
  CHECK(nl_pcall(L, 0, 1) != 0);
  CHECK_STR(nl_typeof(L, -1), "TypeError");
  lua_close(L);
}

static void test_ranges(void)
{
  lua_State *L = newstate();
  lua_Number a = 0, b = 0;
  int ex = -1;
  nl_pushrange(L, 1, 10, 0);
  CHECK_STR(nl_typeof(L, -1), "Range");
  nl_gettype(L, "Range");
  CHECK(nl_isa(L, -2, -1));
  lua_pop(L, 1);
  CHECK(nl_torange(L, -1, &a, &b, &ex) == 1 && a == 1 && b == 10 && ex == 0);
  lua_setglobal(L, "r");
  CHECK(donl(L, "pub fn sum\n  let s = 0\n  for i in :r\n    s += i\n  end\n  s\nend\n"
	        "pub fn excl\n  2...5\nend\n", "=ranges") == 0);
  CHECK(callf(L, -1, "sum", 0, 1) == 0);
  CHECK(lua_tonumber(L, -1) == 55);
  lua_pop(L, 1);
  CHECK(callf(L, -1, "excl", 0, 1) == 0);
  CHECK(nl_torange(L, -1, &a, &b, &ex) == 1 && a == 2 && b == 5 && ex == 1);
  lua_pop(L, 1);
  lua_newtable(L);
  CHECK(nl_torange(L, -1, &a, &b, &ex) == 0);
  lua_close(L);
}

/* -- Methods and operators ------------------------------------------------ */

static int c_hit(lua_State *L)
{
  lua_pushfstring(L, "hit:%d", lua_gettop(L));
  return 1;
}

static void test_methods(void)
{
  lua_State *L = newstate();
  luaL_Reg reg[] = { { "poke", c_hit }, { NULL, NULL } };
  /* A C function marked by hand, and the mark reported. */
  lua_pushcfunction(L, c_hit);
  CHECK(nl_ismethod(L, -1) == 0);
  nl_markmethod(L, -1);
  CHECK(nl_ismethod(L, -1) == 1);
  lua_pop(L, 1);
  /* nlL_setmethods marks every function it registers. */
  lua_newtable(L);
  nlL_setmethods(L, reg);
  lua_getfield(L, -1, "poke");
  CHECK(nl_ismethod(L, -1) == 1);
  lua_pop(L, 1);
  lua_setglobal(L, "unit");
  /* A compiled method carries the mark; a plain function does not, nor a
  ** method taken with `!`, which comes bound to its receiver (the raw
  ** method is read with an index).
  */
  CHECK(donl(L, "struct P\n  x: Number = 1\nend\n"
	        "fn P.m\n  @x\nend\n"
	        "pub fn method\n  P[\"m\"]\nend\n"
	        "pub fn bound\n  let o = P { x: 3 }\n  o.m!\nend\n"
	        "pub fn plain\n  fn(a) a end\nend\n"
	        "pub fn obj\n  P { x: 7 }\nend\n"
	        "pub fn through_unit\n  :unit.poke(1, 2)\nend\n", "=methods") == 0);
  CHECK(callf(L, -1, "method", 0, 1) == 0);
  CHECK(nl_ismethod(L, -1) == 1);
  lua_pop(L, 1);
  CHECK(callf(L, -1, "bound", 0, 1) == 0);
  CHECK(nl_ismethod(L, -1) == 0);
  CHECK(lua_pcall(L, 0, 1, 0) == 0 && lua_tonumber(L, -1) == 3);
  lua_pop(L, 1);
  CHECK(callf(L, -1, "plain", 0, 1) == 0);
  CHECK(nl_ismethod(L, -1) == 0);
  lua_pop(L, 1);
  /* A Lua path keeps Lua's rule: no receiver is added. */
  CHECK(callf(L, -1, "through_unit", 0, 1) == 0);
  CHECK_STR(lua_tostring(L, -1), "hit:2");
  lua_pop(L, 1);
  /* nlL_callmember: a method gets the receiver, a plain function does not,
  ** a string receiver always does.
  */
  CHECK(callf(L, -1, "obj", 0, 1) == 0);
  nlL_callmember(L, "m", 0, 1);
  CHECK(lua_tonumber(L, -1) == 7);
  lua_pop(L, 1);
  lua_getglobal(L, "unit");
  lua_pushinteger(L, 5);
  nlL_callmember(L, "poke", 1, 1);
  CHECK_STR(lua_tostring(L, -1), "hit:2");
  lua_pop(L, 1);
  lua_newtable(L);
  lua_pushcfunction(L, c_hit);
  lua_setfield(L, -2, "f");
  lua_pushinteger(L, 5);
  nlL_callmember(L, "f", 1, 1);
  CHECK_STR(lua_tostring(L, -1), "hit:1");
  lua_pop(L, 1);
  lua_pushliteral(L, "abc");
  nlL_callmember(L, "upcase", 0, 1);
  CHECK_STR(lua_tostring(L, -1), "ABC");
  lua_pop(L, 1);
  lua_close(L);
}

static void test_operators(void)
{
  lua_State *L = newstate();
  CHECK(donl(L, "struct V\n  x: Number\nend\n"
	        "fn V.+(r)\n  V { x: @x + r.x }\nend\n"
	        "fn V.coerce(left)\n  return V { x: left }, self\nend\n"
	        "fn V.<(r)\n  @x < r.x\nend\n"
	        "fn V.>(r)\n  @x > r.x + 100\nend\n"
	        "fn V.==(r)\n  @x == r.x\nend\n"
	        "fn V.len\n  @x * 10\nend\n"
	        "pub fn v(x)\n  V { x: x }\nend\n", "=ops") == 0);
  lua_setglobal(L, "V");
  /* nl_arith: numbers, the events Lua 5.1 lacks, a struct operator, coerce. */
  lua_pushinteger(L, 7); lua_pushinteger(L, 2);
  nl_arith(L, NL_OPIDIV);
  CHECK(lua_tonumber(L, -1) == 3);
  lua_pop(L, 1);
  lua_pushinteger(L, 6); lua_pushinteger(L, 3);
  nl_arith(L, NL_OPBAND);
  CHECK(lua_tonumber(L, -1) == 2);
  lua_pop(L, 1);
  lua_pushinteger(L, 5);
  nl_arith(L, NL_OPUNM);
  CHECK(lua_tonumber(L, -1) == -5);
  lua_pop(L, 1);
  lua_pushliteral(L, "a"); lua_pushinteger(L, 1);
  nl_arith(L, NL_OPCONCAT);
  CHECK_STR(lua_tostring(L, -1), "a1");
  lua_pop(L, 1);
  lua_getglobal(L, "V");
  lua_getfield(L, -1, "v"); lua_pushinteger(L, 1); lua_call(L, 1, 1);
  lua_getfield(L, -2, "v"); lua_pushinteger(L, 2); lua_call(L, 1, 1);
  nl_arith(L, NL_OPADD);            /* V{1} + V{2} */
  lua_getfield(L, -1, "x");
  CHECK(lua_tonumber(L, -1) == 3);
  lua_pop(L, 2);
  lua_pushinteger(L, 10);
  lua_getfield(L, -2, "v"); lua_pushinteger(L, 2); lua_call(L, 1, 1);
  nl_arith(L, NL_OPADD);            /* 10 + V{2}: coerce */
  lua_getfield(L, -1, "x");
  CHECK(lua_tonumber(L, -1) == 12);
  lua_pop(L, 2);
  /* nl_compare: numbers, strings, the struct's <, >, == and len. */
  lua_pushinteger(L, 1); lua_pushinteger(L, 2);
  CHECK(nl_compare(L, -2, -1, NL_OPLT) == 1);
  CHECK(nl_compare(L, -2, -1, NL_OPGE) == 0);
  CHECK(nl_compare(L, -2, -1, NL_OPEQ) == 0);
  lua_pop(L, 2);
  lua_getfield(L, -1, "v"); lua_pushinteger(L, 1); lua_call(L, 1, 1);
  lua_getfield(L, -2, "v"); lua_pushinteger(L, 2); lua_call(L, 1, 1);
  CHECK(nl_compare(L, -2, -1, NL_OPLT) == 1);
  CHECK(nl_compare(L, -2, -1, NL_OPGT) == 0);   /* V.> : 1 > 2 + 100 */
  CHECK(nl_compare(L, -1, -2, NL_OPGT) == 0);   /* V.> : 2 > 1 + 100 */
  CHECK(nl_compare(L, -2, -1, NL_OPEQ) == 0);
  lua_getfield(L, -3, "v"); lua_pushinteger(L, 1); lua_call(L, 1, 1);
  CHECK(nl_compare(L, -3, -1, NL_OPEQ) == 1);    /* V{1} == V{1} by V.== */
  lua_pop(L, 1);
  nl_len(L, -2);
  CHECK(lua_tonumber(L, -1) == 10);
  lua_pop(L, 3);
  lua_pushliteral(L, "héllo");
  nl_len(L, -1);
  CHECK(lua_tonumber(L, -1) == 6);
  lua_pop(L, 2);
  lua_close(L);
}

/* -- Exceptions ----------------------------------------------------------- */

static int raises_string(lua_State *L) { return luaL_error(L, "plain lua error"); }
static int raises_nl(lua_State *L) { lua_pushliteral(L, "raised string"); return nl_raise(L); }
static int raises_typed(lua_State *L) { return nlL_raise(L, "TypeError", "amount %d", -3); }
static int raises_unknown(lua_State *L) { return nlL_raise(L, "NoSuchError", "x"); }
static int raises_foreign(lua_State *L)
{
  lua_newuserdata(L, 4);
  luaL_getmetatable(L, "Foreign");
  lua_setmetatable(L, -2);
  return lua_error(L);
}
static int translate(lua_State *L)
{
  if (luaL_testudata(L, 1, "Foreign")) {
    nl_gettype(L, "TypeError");
    lua_newtable(L);
    lua_pushliteral(L, "translated");
    lua_setfield(L, -2, "message");
    nl_new(L, -2);
    return 1;
  }
  lua_pushnil(L);
  return 1;
}

static void test_exceptions(void)
{
  lua_State *L = newstate();
  size_t len;
  lua_pushcfunction(L, raises_string);
  CHECK(nl_pcall(L, 0, 0) == LUA_ERRRUN);
  CHECK_STR(nl_typeof(L, -1), "LuaError");
  lua_getfield(L, -1, "trace");
  CHECK(lua_isstring(L, -1));
  lua_getfield(L, -2, "value");
  CHECK_CONTAINS(lua_tostring(L, -1), "plain lua error");
  lua_settop(L, 0);
  lua_pushcfunction(L, raises_nl);
  CHECK(nl_pcall(L, 0, 0) != 0);
  CHECK_STR(nl_typeof(L, -1), "Exception");
  lua_getfield(L, -1, "message");
  CHECK_STR(lua_tostring(L, -1), "raised string");
  lua_settop(L, 0);
  lua_pushcfunction(L, raises_typed);
  CHECK(nl_pcall(L, 0, 0) != 0);
  CHECK_STR(nl_typeof(L, -1), "TypeError");
  lua_getfield(L, -1, "message");
  CHECK_STR(lua_tostring(L, -1), "amount -3");
  lua_pop(L, 1);
  CHECK(nlL_tolstring(L, -1, &len) != NULL && len > 0);
  CHECK_CONTAINS(lua_tostring(L, -1), "amount -3");
  lua_settop(L, 0);
  /* An unknown exception type name raises (runtime/nl_50_error.lua). */
  lua_pushcfunction(L, raises_unknown);
  CHECK(nl_pcall(L, 0, 0) != 0);
  CHECK_CONTAINS(errtext(L), "NoSuchError");
  lua_settop(L, 0);
  /* NL try/catch and raise across C. */
  CHECK(donl(L, "pub fn catches\n  try\n    :raise_typed()\n    \"not reached\"\n"
	        "  catch e: TypeError\n    e.message\n  end\nend\n"
	        "pub fn raises\n  raise \"from NL\"\nend\n", "=exc") == 0);
  lua_register(L, "raise_typed", raises_typed);
  CHECK(callf(L, 1, "catches", 0, 1) == 0);
  CHECK_STR(lua_tostring(L, -1), "amount -3");
  lua_pop(L, 1);
  CHECK(callf(L, 1, "raises", 0, 0) != 0);
  nl_gettype(L, "Exception");
  CHECK(nl_isa(L, -2, -1));
  lua_settop(L, 1);
  /* The translator. */
  luaL_newmetatable(L, "Foreign");
  lua_pop(L, 1);
  lua_pushcfunction(L, translate);
  nl_settranslator(L);
  lua_register(L, "raise_foreign", raises_foreign);
  CHECK(donl(L, "pub fn catches_foreign\n  try\n    :raise_foreign()\n    \"not reached\"\n"
	        "  catch e: TypeError\n    e.message\n  end\nend\n", "=translator") == 0);
  CHECK(callf(L, -1, "catches_foreign", 0, 1) == 0);
  CHECK_STR(lua_tostring(L, -1), "translated");
  lua_settop(L, 0);
  lua_pushcfunction(L, raises_foreign);
  CHECK(nl_pcall(L, 0, 0) != 0);
  CHECK_STR(nl_typeof(L, -1), "TypeError");
  lua_close(L);
}

/* -- Tasks ---------------------------------------------------------------- */

static void test_tasks(void)
{
  lua_State *L = newstate();
  int t, w;
  CHECK(donl(L, "pub async fn twice(t)\n  let v = t.await\n  v * 2\nend\n", "=tasks") == 0);
  nl_newtask(L);
  t = lua_gettop(L);
  CHECK_STR(nl_typeof(L, t), "Task");
  CHECK(nl_taskstate(L, t) == NL_TASK_PENDING);
  lua_pushvalue(L, t);
  CHECK(callf(L, 1, "twice", 1, 1) == 0);
  w = lua_gettop(L);
  CHECK(nl_taskstate(L, w) == NL_TASK_PENDING);
  lua_pushnumber(L, 21);
  nl_resolve(L, t);
  /* The waiter ran inside nl_resolve. */
  CHECK(nl_taskstate(L, w) == NL_TASK_DONE);
  CHECK(lua_tonumber(L, -1) == 42);
  lua_pop(L, 1);
  lua_pushnumber(L, 99);
  nl_resolve(L, t);  /* The second time does nothing. */
  CHECK(nl_taskstate(L, t) == NL_TASK_DONE);
  CHECK(lua_tonumber(L, -1) == 21);
  lua_settop(L, 1);
  /* Rejection, with raise's conversion, reaching the awaiting task. */
  nl_newtask(L);
  t = lua_gettop(L);
  lua_pushvalue(L, t);
  CHECK(callf(L, 1, "twice", 1, 1) == 0);
  w = lua_gettop(L);
  lua_pushliteral(L, "went wrong");
  nl_reject(L, t);
  CHECK(nl_taskstate(L, t) == NL_TASK_FAILED);
  CHECK_STR(nl_typeof(L, -1), "Exception");
  lua_pop(L, 1);
  CHECK(nl_taskstate(L, w) == NL_TASK_FAILED);
  lua_getfield(L, -1, "message");
  CHECK_STR(lua_tostring(L, -1), "went wrong");
  lua_settop(L, 1);
  lua_pushnumber(L, 3);
  CHECK(nl_taskstate(L, -1) == -1);
  lua_close(L);
}

typedef struct TimerCall {
  int calls;
  double seconds;
  lua_State *L;
  nl_Callback wake;
  void *arg;
} TimerCall;

static void timer(void *data, lua_State *L, double seconds, nl_Callback wake, void *arg)
{
  TimerCall *t = (TimerCall *)data;
  t->calls++;
  t->seconds = seconds;
  t->L = L;
  t->wake = wake;
  t->arg = arg;
}

static void test_timer(void)
{
  lua_State *L = newstate();
  TimerCall tc;
  int s;
  memset(&tc, 0, sizeof(tc));
  CHECK(donl(L, "pub async fn sleeper(d)\n  Task.sleep(d).await\n  \"slept\"\nend\n", "=timer") == 0);
  lua_pushcfunction(L, openlibs);  /* Any function: the failure below is expected. */
  lua_pop(L, 1);
  CHECK(luaL_loadstring(L, "return") == 0);
  nl_setunhandled(L);
  /* Without a timer, Task.sleep raises: the task fails. */
  lua_pushnumber(L, 1);
  CHECK(callf(L, 1, "sleeper", 1, 1) == 0);
  CHECK(nl_taskstate(L, -1) == NL_TASK_FAILED);
  lua_settop(L, 1);
  nl_settimer(L, timer, &tc);
  lua_pushnumber(L, 0.5);
  CHECK(callf(L, 1, "sleeper", 1, 1) == 0);
  s = lua_gettop(L);
  CHECK(tc.calls == 1 && tc.seconds == 0.5 && tc.L == L);
  CHECK(nl_taskstate(L, s) == NL_TASK_PENDING);
  tc.wake(tc.L, tc.arg);
  CHECK(nl_taskstate(L, s) == NL_TASK_DONE);
  CHECK_STR(lua_tostring(L, -1), "slept");
  lua_settop(L, 1);
  /* Through nl_post, as from a timer thread. */
  lua_pushnumber(L, 2);
  CHECK(callf(L, 1, "sleeper", 1, 1) == 0);
  s = lua_gettop(L);
  CHECK(tc.calls == 2);
  CHECK(nl_post(L, tc.wake, tc.arg) == 1);
  CHECK(nl_taskstate(L, s) == NL_TASK_PENDING);
  CHECK(nl_pump(L) >= 1);
  CHECK(nl_taskstate(L, s) == NL_TASK_DONE);
  lua_settop(L, 1);
  nl_settimer(L, NULL, NULL);
  lua_close(L);
}

static int unhandled_calls;
static char unhandled_msg[256];
static int unhandled_hook(lua_State *L)
{
  unhandled_calls++;
  lua_getfield(L, 1, "message");
  snprintf(unhandled_msg, sizeof(unhandled_msg), "%s", lua_tostring(L, -1) ? lua_tostring(L, -1) : "?");
  return 0;
}

static void post_raises(lua_State *L, void *data)
{
  (void)data;
  if (L) luaL_error(L, "callback raised");
}

static void test_unhandled(void)
{
  lua_State *L = newstate();
  lua_pushcfunction(L, unhandled_hook);
  nl_setunhandled(L);
  unhandled_calls = 0;
  /* A task that fails with nobody waiting: reported at the next pump. */
  nl_newtask(L);
  lua_pushliteral(L, "nobody waits");
  nl_reject(L, 1);
  lua_settop(L, 0);
  CHECK(nl_pump(L) >= 1);
  CHECK(unhandled_calls == 1);
  CHECK_STR(unhandled_msg, "nobody waits");
  /* A posted callback that raises. */
  CHECK(nl_post(L, post_raises, NULL) == 1);
  CHECK(nl_pump(L) >= 1);
  CHECK(unhandled_calls == 2);
  CHECK_CONTAINS(unhandled_msg, "callback raised");
  CHECK(nl_pump(L) == 0);
  lua_close(L);
}

/* An await whose yield fails (across a C call) leaves the task as observed
** as it was: when the task then fails with nothing else waiting, the failure
** is still reported.
*/
static void test_await_failed_yield(void)
{
  lua_State *L = newstate();
  int t;
  lua_pushcfunction(L, unhandled_hook);
  nl_setunhandled(L);
  unhandled_calls = 0;
  CHECK(donl(L,
    "pub async fn waits(t)\n"
    "  try\n"
    "    :string.gsub(\"a\", \".\", fn(c) t.await end)\n"
    "  catch e\n"
    "    :_G.Caught = e.message\n"
    "  end\n"
    "end\n", "=awaitc") == 0);
  nl_newtask(L);
  t = lua_gettop(L);
  lua_pushvalue(L, t);
  CHECK(callf(L, 1, "waits", 1, 1) == 0);
  lua_pop(L, 1);
  CHECK_LUA(L, "type(Caught) == 'string' and Caught:find('yield') ~= nil");
  lua_pushliteral(L, "failed later");
  nl_reject(L, t);
  CHECK(nl_pump(L) >= 1);
  CHECK(unhandled_calls == 1);
  CHECK_STR(unhandled_msg, "failed later");
  lua_close(L);
}

/* -- Events from NL files ------------------------------------------------- */

static void test_events_from_nl(void)
{
  lua_State *L = newstate();
  writefile("ev.nl",
    "on Tick do |n|\n"
    "  :_G.Count = (:_G.Count or 0) + 1\n"
    "end\n"
    "on Tick as 'named' do |n|\n"
    "  return n * 10\n"
    "end\n");
  /* One file, one name however its path is spelled, so its
  ** unnamed handler is replaced on each load, not added.
  */
  CHECK(luaL_dofile(L, "ev.nl") == 0);
  lua_settop(L, 0);
  CHECK(luaL_dofile(L, "ev.nl") == 0);
  lua_settop(L, 0);
  CHECK(luaL_dofile(L, "./ev.nl") == 0);
  lua_settop(L, 0);
  CHECK_LUA(L, "include('ev.nl') ~= false");
  lua_settop(L, 0);
  lua_pushnumber(L, 4);
  nl_runevent(L, "Tick", 1, LUA_MULTRET);
  CHECK(lua_gettop(L) == 1 && lua_tonumber(L, 1) == 40);
  lua_settop(L, 0);
  CHECK_LUA(L, "Count == 1");
  nl_offevent(L, "Tick", "named");
  lua_pushnumber(L, 4);
  CHECK(nl_prunevent(L, "Tick", 1, LUA_MULTRET) == 0);
  CHECK(lua_gettop(L) == 0);
  CHECK_LUA(L, "Count == 2");
  lua_close(L);
}

/* Events 2: a plain handler awaits when the event runs inside a task. */
static void test_handler_awaits(void)
{
  lua_State *L = newstate();
  writefile("aw.nl",
    "let box = {}\n"
    "let pending = Task.new do |resolve, reject|\n"
    "  box.resolve = resolve\n"
    "end\n"
    "on Wait as 'w' do\n"
    "  return pending.await\n"
    "end\n"
    "async fn main_task()\n"
    "  let v = Event.run \"Wait\"\n"
    "  :_G.Got = v\n"
    "end\n"
    "let t = main_task()\n"
    "t.on_error do |e|\n"
    "  :_G.Failed = e.message\n"
    "end\n"
    "box.resolve(42)\n");
  CHECK(luaL_dofile(L, "aw.nl") == 0 || (fprintf(stderr, "  %s\n", errtext(L)), 0));
  lua_settop(L, 0);
  CHECK(nl_pump(L) >= 0);
  CHECK_LUA(L, "Got == 42");
  CHECK_LUA(L, "Failed == nil");
  lua_close(L);
}

/* -- Loading and imports -------------------------------------------------- */

static void test_imports(void)
{
  lua_State *L = newstate();
  mkdir("lib", 0777);
  mkdir("sub", 0777);
  writefile("lib/m.nl", "pub fn f(x)\n  x + 1\nend\n");
  writefile("sub/a.nl", "import { g } from 'b'\npub fn h\n  g()\nend\n");
  writefile("sub/b.nl", "pub fn g\n  \"from sub/b\"\nend\n");
  nl_import(L, "lib/m");
  lua_pushnumber(L, 1);
  CHECK(callf(L, 1, "f", 1, 1) == 0);
  CHECK(lua_tonumber(L, -1) == 2);
  lua_pop(L, 1);
  nl_import(L, "lib/m");
  CHECK(lua_rawequal(L, -1, -2));  /* One run per file. */
  lua_settop(L, 0);
  /* nl_loadbuffer with imports: a file name, and a name that is no file. */
  CHECK(donl(L, "import { f } from 'lib/m'\npub fn g\n  f(10)\nend\n", "@main.nl") == 0);
  CHECK(callf(L, -1, "g", 0, 1) == 0);
  CHECK(lua_tonumber(L, -1) == 11);
  lua_settop(L, 0);
  CHECK(donl(L, "import { f } from 'lib/m'\npub fn g\n  f(20)\nend\n", "=buffer") == 0);
  CHECK(callf(L, -1, "g", 0, 1) == 0);
  CHECK(lua_tonumber(L, -1) == 21);
  lua_settop(L, 0);
  /* Imports relative to the importing file. */
  CHECK(nl_loadfile(L, "sub/a.nl") == 0);
  CHECK(nl_pcall(L, 0, 1) == 0);
  CHECK(callf(L, -1, "h", 0, 1) == 0);
  CHECK_STR(lua_tostring(L, -1), "from sub/b");
  lua_settop(L, 0);
  /* require of an NL file, and of a Lua file that requires it. */
  writefile("lua_side.lua", "local m = require('lib.m'); return { v = m.f(100) }");
  CHECK_LUA(L, "require('lua_side').v == 101");
  /* nl_addpath. */
  mkdir("extra", 0777);
  writefile("extra/far.nl", "pub fn far\n  \"far\"\nend\n");
  nl_addpath(L, "extra");
  nl_import(L, "far");
  CHECK(callf(L, -1, "far", 0, 1) == 0);
  CHECK_STR(lua_tostring(L, -1), "far");
  lua_close(L);
}

/* A host that keeps rt (as Garry's Mod's lua_shared does, for rt.forget):
** the start function of nl_sethostinit gets it, here as the global RT.
*/
static int keep_rt(lua_State *L)
{
  lua_pushvalue(L, 1);
  lua_setglobal(L, "RT");
  return 0;
}

static int host_keeps_rt(lua_State *L)
{
  lua_pushcfunction(L, keep_rt);
  return 1;
}

/* rt.forget: a file and its importers, but not a file that imported it
** once and no longer does.
*/
static void test_forget(void)
{
  lua_State *L = luaL_newstate();
  nl_setreporter(L, reporter, NULL);
  nl_sethostinit(L, host_keeps_rt);
  lua_pushcfunction(L, openlibs);
  CHECK(lua_pcall(L, 0, 0, 0) == 0);
  CHECK_LUA(L, "type(RT) == 'table' and type(RT.forget) == 'function'");
  writefile("fl.nl", "pub fn v\n  1\nend\n");
  writefile("fa.nl", "import { v } from './fl'\npub fn get\n  v()\nend\n");
  nl_import(L, "fa");
  CHECK(callf(L, -1, "get", 0, 1) == 0);
  CHECK(lua_tonumber(L, -1) == 1);
  lua_settop(L, 0);
  /* fl's importers are forgotten with it. */
  CHECK_LUA(L, "#RT.forget('fl.nl') == 2");
  nl_import(L, "fa");
  lua_settop(L, 0);
  /* fa no longer imports fl: forgetting fl leaves fa alone. */
  writefile("fa.nl", "pub fn get\n  2\nend\n");
  CHECK_LUA(L, "#RT.forget('fa.nl') == 1");
  nl_import(L, "fa");
  CHECK(callf(L, -1, "get", 0, 1) == 0);
  CHECK(lua_tonumber(L, -1) == 2);
  lua_settop(L, 0);
  CHECK_LUA(L, "#RT.forget('fl.nl') == 1");
  /* It imports fl again: the link is back. */
  writefile("fa.nl", "import { v } from './fl'\npub fn get\n  v() + 10\nend\n");
  CHECK_LUA(L, "#RT.forget('fa.nl') == 1");
  nl_import(L, "fa");
  CHECK(callf(L, -1, "get", 0, 1) == 0);
  CHECK(lua_tonumber(L, -1) == 11);
  lua_settop(L, 0);
  CHECK_LUA(L, "#RT.forget('fl.nl') == 2");
  lua_close(L);
}

/* Garry's Mod's host (runtime/gmod/host.lua) on the fork, opened as
** lua_shared opens it: its fields over the VM's host table, its start
** called with rt. Its Think hook is the only pump: what workers send back
** arrives through it (nl_pump), and it returns nothing. Its include reads
** the global where it is used, so a later detour counts.
*/
#ifdef NL_ROOT
static int gmod_start(lua_State *L)
{
  lua_pushvalue(L, lua_upvalueindex(1));
  lua_pushvalue(L, 1);
  lua_call(L, 1, 0);
  return 0;
}

static int gmod_hostinit(lua_State *L)
{
  static const char *const fields[] = { "skip_globals", "event", "include", NULL };
  int i;
  if (luaL_loadfile(L, NL_ROOT "/runtime/gmod/host.lua") != 0) lua_error(L);
  lua_call(L, 0, 2);
  for (i = 0; fields[i]; i++) {
    lua_getfield(L, -2, fields[i]);
    if (lua_isnil(L, -1)) lua_pop(L, 1); else lua_setfield(L, 1, fields[i]);
  }
  lua_pushcclosure(L, gmod_start, 1);
  return 1;
}

static void test_gmod_host(void)
{
  lua_State *L = luaL_newstate();
  int frame;
  nl_setreporter(L, reporter, NULL);
  nl_setworkers(L, 1);
  CHECK(nl_setoption(L, NL_OPT_OPENLIBS, 0) == 1);
  lua_pushcfunction(L, openlibs);
  CHECK(lua_pcall(L, 0, 0, 0) == 0);
  /* gmod's hook library, as far as the host uses it: a handler that
  ** returns a value ends the run.
  */
  dolua(L,
    "hook = { t = {} }\n"
    "function hook.Add(n, id, f) hook.t[n] = hook.t[n] or {}; hook.t[n][id] = f end\n"
    "function hook.Remove(n, id) if hook.t[n] then hook.t[n][id] = nil end end\n"
    "function hook.Run(n, ...)\n"
    "  for _, f in pairs(hook.t[n] or {}) do\n"
    "    local a = f(...)\n"
    "    if a ~= nil then return a end\n"
    "  end\n"
    "end\n"
    "include = function(path) return 'first ' .. path end\n");
  nl_sethostinit(L, gmod_hostinit);
  lua_pushcfunction(L, luaopen_nl);
  if (lua_pcall(L, 0, 0, 0) != 0) {
    fprintf(stderr, "  luaopen_nl: %s\n", lua_tostring(L, -1));
    CHECK(0);
    lua_close(L);
    return;
  }
  CHECK_LUA(L, "type(hook.t.Think['NL.pump']) == 'function'");
  writefile("gm_spawn.nl",
    "fn square(x: Number): Number\n"
    "  x * x\n"
    "end\n"
    "async fn main\n"
    "  let a = spawn square(7)\n"
    "  :_G.Got = a.await\n"
    "end\n"
    "unless Worker.parent\n"
    "  main()\n"
    "end\n");
  CHECK(nl_loadfile(L, "gm_spawn.nl") == 0);
  CHECK(nl_pcall(L, 0, 0) == 0 || (fprintf(stderr, "  %s\n", errtext(L)), 0));
  lua_settop(L, 0);
  for (frame = 0; frame < 5000; frame++) {
    CHECK_LUA(L, "hook.Run('Think') == nil");
    lua_getglobal(L, "Got");
    if (!lua_isnil(L, -1)) { lua_pop(L, 1); break; }
    lua_pop(L, 1);
    usleep(1000);
  }
  CHECK_LUA(L, "Got == 49");
  /* include: the global as it is when NL's include runs. */
  CHECK(donl(L, "pub fn inc\n  include(\"x.lua\")\nend\n", "=gm_include") == 0);
  dolua(L, "include = function(path) return 'second ' .. path end");
  CHECK(callf(L, -1, "inc", 0, 1) == 0);
  CHECK_STR(lua_tostring(L, -1), "second x.lua");
  lua_close(L);
}
#endif

/* The intrinsic table of KINTR is out of scripts' reach: no table in the
** registry holds the String library at an index (a script could resize or
** free it), and NL code finds it after full collections.
*/
static void test_intrinsics_out_of_reach(void)
{
  lua_State *L = newstate();
  CHECK_LUA(L,
    "(function()\n"
    "  for _, v in pairs(debug.getregistry()) do\n"
    "    if type(v) == 'table' then\n"
    "      for i, x in pairs(v) do\n"
    "        if x == String and type(i) == 'number' then return false end\n"
    "      end\n"
    "    end\n"
    "  end\n"
    "  return true\n"
    "end)()");
  dolua(L, "collectgarbage() collectgarbage()");
  CHECK(donl(L, "pub fn up(s: String): String\n  s.upcase()\nend\n", "=intr") == 0);
  dolua(L, "collectgarbage() collectgarbage()");
  lua_pushliteral(L, "abc");
  CHECK(callf(L, -2, "up", 1, 1) == 0);
  CHECK_STR(lua_tostring(L, -1), "ABC");
  lua_close(L);
}

/* nl_adddecl: a declared name resolves in files compiled
** after the call; without a host loader, every .d.nl on the search path is
** read as well; a name that cannot be read fails the compile.
*/
static void test_adddecl(void)
{
  lua_State *L = newstate();
  const char *use = "pub fn g\n  host_hello()\nend\n";
  const char *use2 = "pub fn h\n  host_bye()\nend\n";
  dolua(L, "function host_hello() return 'hello' end; function host_bye() return 'bye' end");
  writefile("game.d.nl", "declare fn host_hello(): String\n");
  /* Not declared yet: an undeclared name. */
  ndiag = 0;
  CHECK(nl_loadbuffer(L, use, strlen(use), "=use") != 0);
  CHECK_CONTAINS(lua_tostring(L, -1), "host_hello");
  lua_settop(L, 0);
  nl_adddecl(L, "game.d.nl");
  nl_adddecl(L, "game.d.nl");  /* Twice is once. */
  CHECK(donl(L, use, "=use") == 0);
  CHECK(callf(L, -1, "g", 0, 1) == 0);
  CHECK_STR(lua_tostring(L, -1), "hello");
  lua_settop(L, 0);
  /* A .d.nl in a search-path directory is read without nl_adddecl. */
  mkdir("decls", 0777);
  writefile("decls/more.d.nl", "declare fn host_bye(): String\n");
  CHECK(nl_loadbuffer(L, use2, strlen(use2), "=use2") != 0);
  lua_settop(L, 0);
  nl_addpath(L, "decls");
  CHECK(donl(L, use2, "=use2") == 0);
  CHECK(callf(L, -1, "h", 0, 1) == 0);
  CHECK_STR(lua_tostring(L, -1), "bye");
  lua_settop(L, 0);
  /* A declaration file that cannot be read fails every compile. */
  nl_adddecl(L, "nowhere.d.nl");
  ndiag = 0;
  CHECK(nl_loadbuffer(L, use, strlen(use), "=use") != 0);
  printf("# message: %s\n", lua_tostring(L, -1));
  CHECK_CONTAINS(lua_tostring(L, -1), "nowhere.d.nl");
  CHECK(ndiag >= 1);
  lua_close(L);
}

static void test_compile_errors(void)
{
  lua_State *L = newstate();
  const char *bad = "let = 1\n";
  ndiag = 0;
  CHECK(nl_loadbuffer(L, bad, strlen(bad), "=bad") == LUA_ERRSYNTAX);
  printf("# message: %s\n", lua_tostring(L, -1));
  CHECK_CONTAINS(lua_tostring(L, -1), "bad:1:");
  lua_pop(L, 1);
  CHECK(ndiag >= 1);
  /* A source string as its own name (nlL_loadstring). */
  CHECK(nlL_loadstring(L, "let ok = 1\nlet = 2") == LUA_ERRSYNTAX);
  printf("# message: %s\n", lua_tostring(L, -1));
  CHECK_CONTAINS(lua_tostring(L, -1), "[string \"let ok = 1...\"]:2:");
  lua_pop(L, 1);
  lua_close(L);
}

static void test_checks_option(void)
{
  lua_State *L = newstate();
  const char *src = "pub fn f(x: Number)\n  x\nend\n";
  CHECK(donl(L, src, "=checked") == 0);
  lua_pushliteral(L, "a string");
  CHECK(callf(L, -2, "f", 1, 1) != 0);
  CHECK_STR(nl_typeof(L, -1), "TypeError");
  lua_settop(L, 0);
  CHECK(nl_setoption(L, NL_OPT_CHECKS, 0) == 1);
  CHECK(donl(L, src, "=unchecked") == 0);
  lua_pushliteral(L, "a string");
  CHECK(callf(L, -2, "f", 1, 1) == 0);
  CHECK_STR(lua_tostring(L, -1), "a string");
  lua_close(L);
}

/* NL_OPT_LOCALIZE: at 1, the default, a file
** reads `:string.upper` once, when it loads, and does not see a later
** replacement; at 0 it reads it at each use; at 2 it also reads a global
** alone once, so one created later stays nil for it.
*/
static void test_localize_option(void)
{
  static const char *const want[3][2] = {
    {"late A", "5"}, {"A", "5"}, {"A", "nil"}
  };
  const char *src = "pub fn up(s)\n  :string.upper(s)\nend\n"
		    "pub fn late\n  tostring(:LateValue)\nend\n";
  int level;
  lua_State *L = newstate();
  CHECK(nl_getoption(L, NL_OPT_LOCALIZE) == 1);
  CHECK(nl_setoption(L, NL_OPT_LOCALIZE, 3) == 0);
  CHECK(nl_setoption(L, NL_OPT_LOCALIZE, -1) == 0);
  CHECK(nl_getoption(L, NL_OPT_LOCALIZE) == 1);
  lua_close(L);
  for (level = 0; level <= 2; level++) {
    L = newstate();
    CHECK(nl_setoption(L, NL_OPT_LOCALIZE, level) == 1);
    CHECK(nl_getoption(L, NL_OPT_LOCALIZE) == level);
    CHECK(donl(L, src, "=localize") == 0);
    CHECK(dolua(L, "local u = string.upper\n"
		   "string.upper = function(s) return 'late ' .. u(s) end\n"
		   "LateValue = 5") == 0);
    lua_pushliteral(L, "a");
    CHECK(callf(L, 1, "up", 1, 1) == 0);
    CHECK_STR(lua_tostring(L, -1), want[level][0]);
    lua_pop(L, 1);
    CHECK(callf(L, 1, "late", 0, 1) == 0);
    CHECK_STR(lua_tostring(L, -1), want[level][1]);
    lua_close(L);
  }
}

/* An in-memory loader. */
static int mem_reads;
static const char *mem_file(const char *name)
{
  if (strcmp(name, "mem/a") == 0) return "import { b } from 'b'\npub fn a\n  b() + 1\nend\n";
  if (strcmp(name, "mem/b") == 0) return "pub fn b\n  41\nend\n";
  return NULL;
}
static int mem_resolve(void *d, const char *from, const char *path, char *out, size_t size)
{
  char name[256];
  (void)d; (void)from;
  snprintf(name, sizeof(name), "mem/%s", path);
  if (!mem_file(name)) return 0;
  snprintf(out, size, "%s", name);
  return 1;
}
static const char *mem_read(void *d, const char *name, size_t *size)
{
  const char *s = mem_file(name);
  (void)d;
  if (!s) return NULL;
  mem_reads++;
  *size = strlen(s);
  return s;
}

static void test_loader(void)
{
  lua_State *L = newstate();
  nl_Loader ld;
  ld.resolve = mem_resolve;
  ld.read = mem_read;
  ld.release = NULL;
  ld.data = NULL;
  nl_setloader(L, &ld);
  mem_reads = 0;
  nl_import(L, "a");
  CHECK(callf(L, -1, "a", 0, 1) == 0);
  CHECK(lua_tonumber(L, -1) == 42);
  CHECK(mem_reads >= 2);
  lua_close(L);
}

/* -- Out of memory while bytecode loads ------------------------------------------ */

/* An allocator that fails the failat-th growth after it is armed. It wraps
** the state's own allocator (f, ud): memory from realloc can lie above the
** 47 bits of address a GC64 state needs, as on arm64 Linux. */
typedef struct FailAlloc { lua_Alloc f; void *ud; long n, failat; } FailAlloc;

static void *failalloc(void *ud, void *ptr, size_t osize, size_t nsize)
{
  FailAlloc *fa = (FailAlloc *)ud;
  if (fa->failat > 0 && nsize > osize && ++fa->n == fa->failat) return NULL;
  return fa->f(fa->ud, ptr, osize, nsize);
}

typedef struct DumpBuf { char data[4096]; size_t len; } DumpBuf;

static int dumpwriter(lua_State *L, const void *p, size_t sz, void *ud)
{
  DumpBuf *b = (DumpBuf *)ud;
  (void)L;
  if (b->len + sz > sizeof(b->data)) return 1;
  memcpy(b->data + b->len, p, sz);
  b->len += sz;
  return 0;
}

/* lua_load of bytecode whose main function has upvalues makes its closure
** with lj_func_newL_empty. Every allocation that loading makes fails in turn
** here: each must give LUA_ERRMEM and leave the GC's byte count right (the
** assertion build checks it when the state closes).
*/
static void test_load_bytecode_oom(void)
{
  DumpBuf bc;
  FailAlloc fa;
  lua_State *L = luaL_newstate();
  long k;
  int failed = 0;
  bc.len = 0;
  CHECK(luaL_loadstring(L, "local a, b, c = 1, 2, 3 return function() return a + b + c end") == 0);
  CHECK(lua_pcall(L, 0, 1, 0) == 0);
  CHECK(lua_dump(L, dumpwriter, &bc) == 0);
  lua_close(L);
  for (k = 1; k < 10000; k++) {
    int st;
    L = luaL_newstate();
    if (!L) { CHECK(0); return; }
    fa.f = lua_getallocf(L, &fa.ud);
    fa.n = 0;
    fa.failat = k;
    lua_setallocf(L, failalloc, &fa);
    st = luaL_loadbuffer(L, bc.data, bc.len, "=bc");
    lua_setallocf(L, fa.f, fa.ud);  /* lua_close releases the state's own allocator */
    if (st == 0) {  /* nothing failed: every point was tried */
      lua_close(L);
      break;
    }
    CHECK(st == LUA_ERRMEM);
    failed++;
    lua_close(L);
  }
  printf("# bytecode with upvalues, each allocation of its load failing in turn: %d failed cleanly\n", failed);
  CHECK(failed > 0);
}

int main(int argc, char **argv)
{
  if (argc > 1 && chdir(argv[1]) != 0) { perror(argv[1]); return 2; }
  NL_VERSION_SYM();
  RUN_TEST(test_open);
  RUN_TEST(test_open_late);
  RUN_TEST(test_builtin_types);
  RUN_TEST(test_host_types);
  RUN_TEST(test_structs_and_new);
  RUN_TEST(test_new_checks_fields);
  RUN_TEST(test_keyed);
  RUN_TEST(test_ranges);
  RUN_TEST(test_methods);
  RUN_TEST(test_operators);
  RUN_TEST(test_exceptions);
  RUN_TEST(test_tasks);
  RUN_TEST(test_timer);
  RUN_TEST(test_unhandled);
  RUN_TEST(test_await_failed_yield);
  RUN_TEST(test_events_from_nl);
  RUN_TEST(test_handler_awaits);
  RUN_TEST(test_imports);
  RUN_TEST(test_forget);
#ifdef NL_ROOT
  RUN_TEST(test_gmod_host);
#endif
  RUN_TEST(test_intrinsics_out_of_reach);
  RUN_TEST(test_adddecl);
  RUN_TEST(test_compile_errors);
  RUN_TEST(test_checks_option);
  RUN_TEST(test_localize_option);
  RUN_TEST(test_loader);
  RUN_TEST(test_load_bytecode_oom);
  return check_summary("test_capi_real");
}
