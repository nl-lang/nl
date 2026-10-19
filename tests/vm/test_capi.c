/*
** Tests of the functions of nl.h, against the fake runtime
** (tests/vm/fake_runtime) and the identity compiler
** (tests/vm/support/nlc_identity.c). Run by tests/vm/run.sh with a scratch
** directory as argv[1]; the test works inside it.
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

/* -- Helpers -------------------------------------------------------------- */

static lua_State *newstate(void)
{
  lua_State *L = luaL_newstate();
  luaL_openlibs(L);
  return L;
}

static void writefile(const char *name, const char *text)
{
  FILE *fp = fopen(name, "wb");
  if (!fp) { perror(name); exit(2); }
  fputs(text, fp);
  fclose(fp);
}

/* Runs Lua code that must not fail; returns the number of results. */
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

/* Evaluates a Lua expression that must be true. */
#define CHECK_LUA(L, expr) do { \
  int n_ = dolua(L, "return " expr); \
  check_count++; \
  if (n_ < 1 || !lua_toboolean(L, -1)) { \
    check_failed_total++; check_test_failed = 1; \
    fprintf(stderr, "%s:%d: CHECK_LUA failed: %s\n", __FILE__, __LINE__, expr); \
  } \
  lua_pop(L, n_); \
} while (0)

/* The type of the value on top as seen by the fake runtime. */
static const char *typename_top(lua_State *L)
{
  return nl_typeof(L, -1);
}

/* -- luaopen_nl ----------------------------------------------------------- */

static void test_openlibs_opens_nl(void)
{
  lua_State *L = newstate();
  CHECK_LUA(L, "type(event) == 'table' and type(event.run) == 'function'");
  CHECK_LUA(L, "__fake_rt.fake == true");
  CHECK_LUA(L, "__fake_rt.event_was_c == true");  /* C library opened first. */
  CHECK_LUA(L, "type(include) == 'function' and type(typeof) == 'function'");
  CHECK_LUA(L, "package.loaded.event == event");
  /* A second luaopen_nl does nothing. */
  dolua(L, "__fake_rt.marker = 1");
  luaopen_nl(L);
  CHECK_LUA(L, "__fake_rt.marker == 1");
  CHECK(lua_gettop(L) == 0);
  lua_close(L);
}

static void test_luaopen_nl_by_hand(void)
{
  lua_State *L = luaL_newstate();
  lua_pushcfunction(L, luaopen_base); lua_call(L, 0, 0);
  lua_pushcfunction(L, luaopen_string); lua_call(L, 0, 0);
  lua_pushcfunction(L, luaopen_table); lua_call(L, 0, 0);
  lua_pushcfunction(L, luaopen_math); lua_call(L, 0, 0);
  lua_pushcfunction(L, luaopen_bit); lua_call(L, 0, 0);
  lua_pushcfunction(L, luaopen_debug); lua_call(L, 0, 0);
  lua_pushcfunction(L, luaopen_io); lua_call(L, 0, 0);
  lua_pushnumber(L, 1);
  CHECK(nl_typeof(L, -1) == NULL);  /* NL not open: no runtime to ask. */
  lua_pop(L, 1);
  lua_pushcfunction(L, luaopen_nl);
  CHECK(lua_pcall(L, 0, 0, 0) == 0);
  lua_pushnumber(L, 1);
  CHECK_STR(nl_typeof(L, -1), "Number");
  lua_pop(L, 1);
  CHECK_LUA(L, "type(event) == 'table'");
  lua_close(L);
}

/* -- Options -------------------------------------------------------------- */

static int diag_count;
static char diag_last[512];
static int diag_sev[16];
static char diag_codes[16][64];

static void reporter(void *data, const nl_Diagnostic *d)
{
  int *calls = (int *)data;
  if (calls) (*calls)++;
  if (diag_count < 16) {
    diag_sev[diag_count] = d->severity;
    snprintf(diag_codes[diag_count], sizeof(diag_codes[0]), "%s", d->code);
  }
  diag_count++;
  snprintf(diag_last, sizeof(diag_last), "%s:%d:%d: %s", d->file, d->line,
	   d->column, d->message);
}

static void test_options(void)
{
  lua_State *L = newstate();
  CHECK(nl_getoption(L, NL_OPT_CHECKS) == 1);
  CHECK(nl_getoption(L, NL_OPT_STRIP) == 0);
  CHECK(nl_getoption(L, NL_OPT_MAXWORKERS) == 64);
  CHECK(nl_getoption(L, NL_OPT_WORKERMEM) == 0);
  CHECK(nl_getoption(L, NL_OPT_GLUA) == 0);
  CHECK(nl_getoption(L, NL_OPT_SANDBOX) == 0);
  CHECK(nl_getoption(L, NL_OPT_LOCALIZE) == 1);
  CHECK(nl_setoption(L, NL_OPT_LOCALIZE, 3) == 0);
  CHECK(nl_setoption(L, NL_OPT_LOCALIZE, -1) == 0);
  CHECK(nl_setoption(L, NL_OPT_LOCALIZE, 2) == 1);
  CHECK(nl_setoption(L, NL_OPT_CHECKS, 0) == 1);
  CHECK(nl_setoption(L, NL_OPT_STRIP, 1) == 1);
  CHECK(nl_setoption(L, NL_OPT_CHECKS, 2) == 0);
  CHECK(nl_setoption(L, NL_OPT_MAXWORKERS, -1) == 0);
  CHECK(nl_setoption(L, NL_OPT_WORKERMEM, 1 << 20) == 1);
  CHECK(nl_setoption(L, NL_OPT_GLUA, 2) == 0);
  CHECK(nl_setoption(L, NL_OPT_GLUA, 1) == 1);
  CHECK(nl_getoption(L, NL_OPT_GLUA) == 1);
  CHECK(nl_setoption(L, 99, 1) == 0);
  CHECK(nl_getoption(L, 99) == 0);
  CHECK(nl_getoption(L, NL_OPT_CHECKS) == 0);
  CHECK(nl_isworker(L) == 0);
  /* The options reach the compiler. */
  diag_count = 0;
  nl_setreporter(L, reporter, NULL);
  CHECK(nlL_loadstring(L, "--@options\nreturn 1") == 0);
  lua_pop(L, 1);
  CHECK(diag_count == 1);
  CHECK_CONTAINS(diag_last, "checks=0 strip=1 localize=2");
  lua_close(L);
}

/* NL_OPT_OPENLIBS 0: luaL_openlibs leaves NL out, and luaopen_nl opens it
** later. nl_sethostinit's function sees the host table before the runtime,
** and what it returns gets rt.
*/
static int hostinit_calls, hoststart_calls;
static int hoststart(lua_State *L)
{
  hoststart_calls++;
  CHECK(lua_istable(L, 1));
  lua_getfield(L, 1, "fake");
  CHECK(lua_toboolean(L, -1));
  return 0;
}

static int hostinit(lua_State *L)
{
  hostinit_calls++;
  CHECK(lua_istable(L, 1));
  lua_getfield(L, 1, "compile");
  CHECK(lua_isfunction(L, -1));
  lua_getfield(L, 1, "loadstring");
  CHECK(lua_isfunction(L, -1));
  lua_pop(L, 2);
  CHECK(hoststart_calls == 0);
  lua_pushcfunction(L, hoststart);
  return 1;
}

static void test_openlibs_option(void)
{
  lua_State *L = luaL_newstate();
  CHECK(nl_getoption(L, NL_OPT_OPENLIBS) == 1);
  CHECK(nl_setoption(L, NL_OPT_OPENLIBS, 2) == 0);
  CHECK(nl_setoption(L, NL_OPT_OPENLIBS, 0) == 1);
  luaL_openlibs(L);
  CHECK_LUA(L, "event == nil and include == nil and typeof == nil");
  CHECK_LUA(L, "type(string) == 'table' and type(jit) == 'table'");
  /* A dump of GLua code is Lua bytecode: it loads where NL is not open. */
  CHECK(nl_setoption(L, NL_OPT_GLUA, 1) == 1);
  CHECK_LUA(L, "loadstring(string.dump(function() return 7 end))() == 7");
  hostinit_calls = hoststart_calls = 0;
  nl_sethostinit(L, hostinit);
  lua_pushcfunction(L, luaopen_nl);
  CHECK(lua_pcall(L, 0, 0, 0) == 0);
  CHECK(hostinit_calls == 1 && hoststart_calls == 1);
  CHECK_LUA(L, "__fake_rt.fake == true and type(event) == 'table'");
  /* Open already: nothing more. */
  lua_pushcfunction(L, luaopen_nl);
  CHECK(lua_pcall(L, 0, 0, 0) == 0);
  CHECK(hostinit_calls == 1 && hoststart_calls == 1);
  CHECK(lua_gettop(L) == 0);
  lua_close(L);
}

/* GLua code (parsed with NL_OPT_GLUA) keeps Lua 5.1's rules for __len and
** ordered comparisons, in the interpreter and in traces, as Garry's Mod has
** them. Other Lua code has NL's (DESIGN 14.2).
*/
static void test_glua_metamethod_rules(void)
{
  lua_State *L = newstate();
  CHECK(nl_setoption(L, NL_OPT_GLUA, 1) == 1);
  dolua(L, "lenmt = { __len = function() return 42 end } "
	   "ltmt = { __lt = function() return true end, __le = function() return true end } "
	   "othermt = { __lt = function() return true end }");
  CHECK_LUA(L, "#setmetatable({ 1, 2 }, lenmt) == 2");
  CHECK_LUA(L, "(function() local t, n = setmetatable({ 1, 2 }, lenmt), 0 "
	       "for i = 1, 300 do n = n + #t end return n end)() == 600");
  CHECK_LUA(L, "not pcall(function() return setmetatable({}, ltmt) < 1 end)");
  CHECK_LUA(L, "not pcall(function() return 1 <= setmetatable({}, ltmt) end)");
  CHECK_LUA(L, "not pcall(function() return setmetatable({}, ltmt) < setmetatable({}, othermt) end)");
  CHECK_LUA(L, "setmetatable({}, ltmt) < setmetatable({}, ltmt)");
  CHECK_LUA(L, "(function() local a, b, n = setmetatable({}, ltmt), setmetatable({}, ltmt), 0 "
	       "for i = 1, 300 do if a < b then n = n + 1 end end return n end)() == 300");
  CHECK_LUA(L, "(function() local a, n = setmetatable({}, ltmt), 0 "
	       "for i = 1, 300 do if pcall(function() return a < i end) then n = n + 1 end end return n end)() == 0");
  /* Lua code that is not GLua: NL's rules. */
  CHECK(nl_setoption(L, NL_OPT_GLUA, 0) == 1);
  CHECK_LUA(L, "#setmetatable({ 1, 2 }, lenmt) == 42");
  CHECK_LUA(L, "setmetatable({}, ltmt) < 1");
  lua_close(L);
}

/* Loads Lua source; returns 0, or 1 with the message in glua_err. */
static char glua_err[256];
static int glua_load(lua_State *L, const char *src)
{
  int st = luaL_loadstring(L, src);
  glua_err[0] = 0;
  if (st) snprintf(glua_err, sizeof(glua_err), "%s", lua_tostring(L, -1));
  lua_pop(L, 1);
  return st != 0;
}

/* NL_OPT_GLUA: Lua chunks loaded while it is set are GLua.
** Upstream LuaJIT already has `&&`, `||`, `!`, `!=` and a soft `continue`;
** the option adds C comments and makes `continue` a reserved word.
*/
static void test_glua(void)
{
  lua_State *L = newstate();
  static const char *const glua_only[] = {
    "local x = 1 // comment\nreturn x",
    "return 1 /* comment */ + 2 /**/ + 3 /* ** */ + 4 /* * / */ /***/",
    "for i = 1, 3 do if i == 2 then continue print(i) end end",
    NULL
  };
  int i;
  /* Off (the default): C comments and a hard `continue` are errors, the
  ** syntax upstream has is not.
  */
  for (i = 0; glua_only[i]; i++) CHECK(glua_load(L, glua_only[i]));
  CHECK(!glua_load(L, "return 1 != 2 && true || !false"));
  CHECK(!glua_load(L, "for i = 1, 3 do if i == 2 then continue end end"));
  CHECK(!glua_load(L, "for i = 1, 3 do if i == 2 then continue; end end"));
  CHECK(!glua_load(L, "local continue = 1 return continue"));
  /* On. */
  CHECK(nl_setoption(L, NL_OPT_GLUA, 1) == 1);
  for (i = 0; glua_only[i]; i++) {
    CHECK(!glua_load(L, glua_only[i]));
    if (glua_err[0]) fprintf(stderr, "  %s\n", glua_err);
  }
  CHECK_LUA(L, "(function() local x = 4 // 2\n return x end)() == 4");
  CHECK_LUA(L, "(function() local x = 8 / 2 x /= 2 return x end)() == 2");
  CHECK_LUA(L, "#'a//b /* c */' == 12");
  CHECK_LUA(L, "(function() local n = 0 for i = 1, 5 do "
	       "if i % 2 == 0 then continue; end n = n + i end return n end)() == 9");
  CHECK_LUA(L, "(function() local n, i = 0, 0 repeat i = i + 1 "
	       "if i == 3 then continue end n = n + i until i >= 5 return n end)() == 12");
  CHECK_LUA(L, "(function() local n, i = 0, 0 while i < 5 do i = i + 1 "
	       "if i == 3 then continue end n = n + i end return n end)() == 12");
  CHECK_LUA(L, "(function() local t = {} for i = 1, 3 do local x = i "
	       "if x == 2 then continue end t[#t+1] = function() return x end end "
	       "return t[1]() == 1 and t[2]() == 3 end)()");
  /* Lines are counted through a C comment. */
  CHECK(glua_load(L, "/* a\nb\nc */ x = = 1"));
  CHECK_CONTAINS(glua_err, ":3:");
  /* An unfinished C comment runs to the end of the chunk, as in gmod. */
  CHECK(!glua_load(L, "local x = 1 /* open"));
  CHECK(!glua_load(L, "local x = 1 /* open *"));
  /* `continue` is a reserved word, not a name. */
  CHECK(glua_load(L, "local continue = 1 return continue"));
  CHECK(glua_load(L, "local t = {} t.continue = 1"));
  CHECK(glua_load(L, "for i = 1, 2 do ::continue:: end"));
  CHECK(glua_load(L, "continue"));
  CHECK_CONTAINS(glua_err, "no loop to continue");
  /* The until expression may not see a local declared after the jump. */
  CHECK(glua_load(L, "local i = 0 repeat i = i + 1 if i < 3 then continue end "
		     "local done = i >= 3 until done"));
  CHECK_CONTAINS(glua_err, "jumps into the scope of local 'done'");
  /* Off again: the next chunk is Lua. */
  CHECK(nl_setoption(L, NL_OPT_GLUA, 0) == 1);
  CHECK(glua_load(L, glua_only[0]));
  CHECK(lua_gettop(L) == 0);
  lua_close(L);
}

/* -- Loading -------------------------------------------------------------- */

static void test_loadbuffer(void)
{
  lua_State *L = newstate();
  const char *src = "local a, b = ...\nreturn { sum = a + b, rt = __nl_rt ~= nil }";
  CHECK(nl_loadbuffer(L, src, strlen(src), "=sum") == 0);
  CHECK(lua_isfunction(L, -1));
  lua_pushnumber(L, 2);
  lua_pushnumber(L, 3);
  CHECK(lua_pcall(L, 2, 1, 0) == 0);
  lua_getfield(L, -1, "sum");
  CHECK(lua_tonumber(L, -1) == 5);
  lua_getfield(L, -2, "rt");
  CHECK(lua_toboolean(L, -1));  /* Loaded through the chunk protocol. */
  lua_settop(L, 0);
  /* nlL_dostring. */
  CHECK(nlL_dostring(L, "return 7") == 0);
  CHECK(lua_tonumber(L, -1) == 7);
  lua_settop(L, 0);
  lua_close(L);
}

static void test_compile_errors(void)
{
  lua_State *L = newstate();
  const char *bad = "--@warning shadow w1\n--@error undeclared-name y is not declared\n"
		    "--@lint naming l1\n--@error other second\n--@error other third\n";
  int calls = 0;
  /* Without a reporter: errors still come back. */
  CHECK(nl_loadbuffer(L, bad, strlen(bad), "@bad.nl") == LUA_ERRSYNTAX);
  CHECK_STR(lua_tostring(L, -1),
	    "bad.nl:2:1: y is not declared (and 2 more errors)");
  lua_pop(L, 1);
  /* One more error: singular. */
  CHECK(nlL_loadstring(L, "--@error a one\n--@error b two") == LUA_ERRSYNTAX);
  CHECK_CONTAINS(lua_tostring(L, -1), ":1:1: one (and 1 more error)");
  lua_pop(L, 1);
  /* With a reporter: every diagnostic, errors first. */
  diag_count = 0;
  nl_setreporter(L, reporter, &calls);
  CHECK(nl_loadbuffer(L, bad, strlen(bad), "=b") == LUA_ERRSYNTAX);
  lua_pop(L, 1);
  CHECK(calls == 5 && diag_count == 5);
  CHECK(diag_sev[0] == NL_DIAG_ERROR && diag_sev[3] == NL_DIAG_WARNING &&
	diag_sev[4] == NL_DIAG_LINT);
  CHECK_STR(diag_codes[0], "undeclared-name");
  /* Warnings alone do not fail the load. */
  calls = 0;
  CHECK(nlL_loadstring(L, "--@warning w careful\nreturn 1") == 0);
  lua_pop(L, 1);
  CHECK(calls == 1);
  /* A Lua syntax error in the generated chunk is LUA_ERRSYNTAX too. */
  CHECK(nlL_loadstring(L, "return +") == LUA_ERRSYNTAX);
  lua_pop(L, 1);
  nl_setreporter(L, NULL, NULL);
  calls = 0;
  CHECK(nlL_loadstring(L, "--@warning w careful\nreturn 1") == 0);
  CHECK(calls == 0);
  lua_close(L);
}

static void test_choice_by_extension(void)
{
  lua_State *L = newstate();
  const char *src = "--@error marker NL compiler ran\nreturn __nl_rt ~= nil";
  writefile("ext.nl", "return { nl = __nl_rt ~= nil }");
  writefile("ext.lua", "return { nl = __nl_rt ~= nil }");
  writefile("ext.txt", "return { nl = __nl_rt ~= nil }");
  writefile("err.nl", "--@error marker NL compiler ran\nreturn 1");
  /* luaL_loadfile and loadfile/dofile/require/include: by extension. */
  CHECK(luaL_loadfile(L, "ext.nl") == 0);
  CHECK(lua_pcall(L, 0, 1, 0) == 0);
  lua_getfield(L, -1, "nl");
  CHECK(lua_toboolean(L, -1));
  lua_settop(L, 0);
  CHECK(luaL_loadfile(L, "ext.lua") == 0);
  CHECK(lua_pcall(L, 0, 1, 0) == 0);
  lua_getfield(L, -1, "nl");
  CHECK(!lua_toboolean(L, -1));
  lua_settop(L, 0);
  CHECK(luaL_loadfile(L, "err.nl") == LUA_ERRSYNTAX);
  CHECK_STR(lua_tostring(L, -1), "err.nl:1:1: NL compiler ran");
  lua_settop(L, 0);
  CHECK_LUA(L, "dofile('ext.nl').nl == true and dofile('ext.lua').nl == false");
  CHECK_LUA(L, "loadfile('ext.nl')().nl == true");
  CHECK_LUA(L, "include('ext.nl').nl == true and include('ext.lua').nl == false");
  CHECK_LUA(L, "select('#', loadfile('err.nl')) == 2");
  /* require with a ?.nl pattern in package.path uses luaL_loadfile. */
  CHECK_LUA(L, "(function() package.path = './?.nl;' .. package.path; "
	       "return require('ext').nl == true end)()");
  /* Mode: NL source is text. */
  CHECK(luaL_loadfilex(L, "ext.nl", "b") == LUA_ERRSYNTAX);
  CHECK_CONTAINS(lua_tostring(L, -1), "wrong mode");
  lua_pop(L, 1);
  CHECK(luaL_loadfilex(L, "ext.nl", "t") == 0);
  lua_pop(L, 1);
  /* luaL_loadbuffer, luaL_loadstring, lua_load, loadstring: always Lua. */
  CHECK(luaL_loadbuffer(L, src, strlen(src), "@ext.nl") == 0);
  CHECK(lua_pcall(L, 0, 1, 0) == 0);
  CHECK(!lua_toboolean(L, -1));
  lua_settop(L, 0);
  CHECK(luaL_loadstring(L, src) == 0);
  lua_settop(L, 0);
  CHECK_LUA(L, "loadstring('return __nl_rt', '@x.nl')() == nil");
  CHECK_LUA(L, "load('return __nl_rt', '@x.nl')() == nil");
  /* nl_loadfile: NL whatever the extension. */
  CHECK(nl_loadfile(L, "ext.txt") == 0);
  CHECK(lua_pcall(L, 0, 1, 0) == 0);
  lua_getfield(L, -1, "nl");
  CHECK(lua_toboolean(L, -1));
  lua_settop(L, 0);
  CHECK(nl_loadfile(L, "missing.nl") == LUA_ERRFILE);
  CHECK_CONTAINS(lua_tostring(L, -1), "cannot open missing.nl");
  lua_pop(L, 1);
  CHECK(luaL_loadfile(L, "missing.nl") == LUA_ERRFILE);
  lua_pop(L, 1);
  /* include: errors from loading and from running propagate. */
  writefile("raise.nl", "error('boom from include')");
  CHECK_LUA(L, "not pcall(include, 'missing.nl')");
  CHECK_LUA(L, "select(2, pcall(include, 'raise.nl')):find('boom from include') ~= nil");
  CHECK_LUA(L, "select(2, pcall(include, 'err.nl')):find('NL compiler ran') ~= nil");
  /* nlL_dofile. */
  CHECK(nlL_dofile(L, "ext.txt") == 0);
  lua_settop(L, 0);
  lua_close(L);
}

static void test_import_and_search_order(void)
{
  lua_State *L;
  char cwd[4096];
  if (!getcwd(cwd, sizeof(cwd))) { CHECK(0); return; }
  mkdir("imp", 0777);
  mkdir("imp/sub", 0777);
  mkdir("searchA", 0777);
  mkdir("searchB", 0777);
  writefile("imp/sub/here.nl", "return { where = 'importing dir' }");
  writefile("here.nl", "return { where = 'cwd' }");
  writefile("searchA/here.nl", "return { where = 'search' }");
  writefile("cwdonly.nl", "return { where = 'cwd' }");
  writefile("searchA/deep.nl", "return { where = 'searchA' }");
  writefile("searchB/deep.nl", "return { where = 'searchB' }");
  writefile("searchB/onlyb.nl", "return { where = 'searchB', n = (onlyb_runs or 0) + 1 }");
  setenv("NL_PATH", "::searchA::searchB:", 1);  /* Empty entries are skipped. */
  L = newstate();
  unsetenv("NL_PATH");
  /* 1. the importing file's directory, 2. cwd, 3. the search path. */
  CHECK_LUA(L, "__fake_rt.host.resolve('imp/sub/main.nl', 'here') == 'imp/sub/here.nl'");
  CHECK_LUA(L, "__fake_rt.host.resolve(nil, 'here') == 'here.nl'");
  CHECK_LUA(L, "__fake_rt.host.resolve('imp/sub/main.nl', 'cwdonly') == 'cwdonly.nl'");
  CHECK_LUA(L, "__fake_rt.host.resolve(nil, 'deep') == 'searchA/deep.nl'");
  CHECK_LUA(L, "__fake_rt.host.resolve(nil, 'nothing') == nil");
  CHECK_LUA(L, "__fake_rt.host.resolve(nil, 'here.nl') == nil");  /* .nl is added. */
  /* Canonical names are real paths, relative to the working directory
  ** when inside it (as nlc names files): one file, one name.
  */
  CHECK_LUA(L, "__fake_rt.host.resolve(nil, 'imp/../here') == 'here.nl'");
  CHECK_LUA(L, "__fake_rt.host.resolve('./imp/sub/main.nl', 'here') == 'imp/sub/here.nl'");
  /* host.canonical: the file as given, with its extension, no search. */
  CHECK_LUA(L, "__fake_rt.host.canonical('./imp/../here.nl') == 'here.nl'");
  CHECK_LUA(L, "__fake_rt.host.canonical('deep.nl') == nil");  /* No search. */
  CHECK_LUA(L, "__fake_rt.host.canonical('here') == nil");     /* No extension added. */
  CHECK_LUA(L, "__fake_rt.host.canonical('imp') == nil");      /* Not a file. */
  /* nl_import: runs once, pushes the export table. */
  nl_import(L, "onlyb");
  lua_getfield(L, -1, "where");
  CHECK_STR(lua_tostring(L, -1), "searchB");
  lua_pop(L, 1);
  nl_import(L, "onlyb");
  CHECK(lua_rawequal(L, -1, -2));
  lua_settop(L, 0);
  /* nl_addpath: appended; a relative directory is taken against the
  ** working directory at the time of the call.
  */
  mkdir("added", 0777);
  writefile("added/late.nl", "return { where = 'added' }");
  CHECK_LUA(L, "__fake_rt.host.resolve(nil, 'late') == nil");
  nl_addpath(L, "added");
  if (chdir("imp") == 0) {
    CHECK_LUA(L, "__fake_rt.host.resolve(nil, 'late') ~= nil");
    /* Relative NL_PATH entries were resolved when the state was made. */
    CHECK_LUA(L, "__fake_rt.host.resolve(nil, 'deep') ~= nil");
    CHECK(chdir(cwd) == 0);
  }
  CHECK_LUA(L, "add_path ~= nil");
  lua_close(L);
}

/* An in-memory loader. */
typedef struct MemLoader {
  int resolves, reads, releases;
  char lastfrom[256];
} MemLoader;

static const char *mem_file(const char *name)
{
  if (strcmp(name, "mem:a") == 0) return "--@import b\nreturn { a = 1 }";
  if (strcmp(name, "mem:b") == 0) return "return { b = 2 }";
  if (strcmp(name, "main.nl") == 0) return "return { main = true }";
  return NULL;
}

static int mem_resolve(void *data, const char *from, const char *path,
		       char *out, size_t size)
{
  MemLoader *m = (MemLoader *)data;
  char name[256];
  m->resolves++;
  snprintf(m->lastfrom, sizeof(m->lastfrom), "%s", from ? from : "(null)");
  snprintf(name, sizeof(name), "mem:%s", path);
  if (!mem_file(name)) return 0;
  snprintf(out, size, "%s", name);
  return 1;
}

static const char *mem_read(void *data, const char *name, size_t *size)
{
  MemLoader *m = (MemLoader *)data;
  const char *s = mem_file(name);
  char *copy;
  if (!s) return NULL;
  m->reads++;
  copy = strdup(s);
  *size = strlen(copy);
  return copy;
}

static void mem_release(void *data, const char *name, const char *buf)
{
  MemLoader *m = (MemLoader *)data;
  (void)name;
  m->releases++;
  free((void *)buf);
}

static void test_loader(void)
{
  lua_State *L = newstate();
  MemLoader m;
  nl_Loader ld;
  memset(&m, 0, sizeof(m));
  ld.resolve = mem_resolve;
  ld.read = mem_read;
  ld.release = mem_release;
  ld.data = &m;
  nl_setloader(L, &ld);
  memset(&ld, 0, sizeof(ld));  /* The state keeps a copy. */
  nl_import(L, "a");
  lua_getfield(L, -1, "a");
  CHECK(lua_tonumber(L, -1) == 1);
  lua_settop(L, 0);
  /* The runtime resolved and read "a"; the compiler resolved and read "b",
  ** with "mem:a" as the importing file.
  */
  CHECK(m.resolves == 2);
  CHECK(m.reads == 2 && m.releases == 2);
  CHECK_STR(m.lastfrom, "mem:a");
  /* nl_loadfile and luaL_loadfile read .nl files through the loader too. */
  CHECK(nl_loadfile(L, "main.nl") == 0);
  CHECK(lua_pcall(L, 0, 1, 0) == 0);
  lua_getfield(L, -1, "main");
  CHECK(lua_toboolean(L, -1));
  lua_settop(L, 0);
  CHECK(luaL_loadfile(L, "main.nl") == 0);
  lua_settop(L, 0);
  CHECK(m.reads == 4 && m.releases == 4);
  CHECK(nl_loadfile(L, "nowhere.nl") == LUA_ERRFILE);
  lua_settop(L, 0);
  /* A failed import inside the compiler is a compile error. */
  CHECK(nlL_loadstring(L, "--@import nothere\nreturn 1") == LUA_ERRSYNTAX);
  CHECK_CONTAINS(lua_tostring(L, -1), "cannot find import 'nothere'");
  lua_settop(L, 0);
  /* NULL restores the disk. */
  nl_setloader(L, NULL);
  writefile("disk.nl", "return { disk = true }");
  nl_import(L, "disk");
  lua_getfield(L, -1, "disk");
  CHECK(lua_toboolean(L, -1));
  lua_settop(L, 0);
  CHECK(m.reads == 4);
  lua_close(L);
}

/* -- Types and values ----------------------------------------------------- */

static void test_types(void)
{
  lua_State *L = newstate();
  const char *s;
  int i;
  luaL_newmetatable(L, "Unit");
  nl_settype(L, -1, "Unit", 0);
  luaL_newmetatable(L, "Hero");
  nl_settype(L, -1, "Hero", -2);
  lua_settop(L, 0);
  lua_newuserdata(L, 8);
  luaL_getmetatable(L, "Hero");
  lua_setmetatable(L, -2);  /* 1: a Hero */
  lua_newuserdata(L, 8);
  luaL_getmetatable(L, "Unit");
  lua_setmetatable(L, -2);  /* 2: a Unit */
  CHECK_STR(nl_typeof(L, 1), "Hero");
  CHECK_STR(nl_typeof(L, 2), "Unit");
  nl_gettype(L, "Unit");  /* 3 */
  nl_gettype(L, "Hero");  /* 4 */
  CHECK(nl_isa(L, 1, 3) && nl_isa(L, 1, 4));
  CHECK(nl_isa(L, 2, 3) && !nl_isa(L, 2, 4));
  CHECK(nl_isa(L, -4, -2));  /* Relative indices. */
  nl_gettype(L, "NoSuchType");
  CHECK(lua_isnil(L, -1));
  lua_pop(L, 1);
  /* Primitive names; the string outlives the call and a collection. */
  lua_pushnumber(L, 1.5);
  s = nl_typeof(L, -1);
  lua_pop(L, 1);
  for (i = 0; i < 3; i++) lua_gc(L, LUA_GCCOLLECT, 0);
  CHECK_STR(s, "Number");
  lua_pushliteral(L, "x");
  CHECK_STR(typename_top(L), "String");
  lua_pop(L, 1);
  CHECK(lua_gettop(L) == 4);
  /* nl_new: pops the fields, pushes the instance. */
  nl_gettype(L, "Exception");
  lua_newtable(L);
  lua_pushliteral(L, "made by nl_new");
  lua_setfield(L, -2, "message");
  nl_new(L, -2);
  CHECK(lua_gettop(L) == 6);
  CHECK_STR(nl_typeof(L, -1), "Exception");
  CHECK(nl_isa(L, -1, -2));
  lua_getfield(L, -1, "message");
  CHECK_STR(lua_tostring(L, -1), "made by nl_new");
  lua_settop(L, 0);
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

static void test_settype_twice_raises(void)
{
  lua_State *L = newstate();
  lua_pushcfunction(L, settype_twice);
  CHECK(lua_pcall(L, 0, 0, 0) != 0);
  CHECK_CONTAINS(lua_tostring(L, -1), "already registered");
  lua_close(L);
}

static void test_ranges(void)
{
  lua_State *L = newstate();
  lua_Number a = 0, b = 0;
  int ex = -1;
  nl_pushrange(L, 1, 10, 0);
  CHECK(nl_torange(L, -1, &a, &b, &ex) == 1);
  CHECK(a == 1 && b == 10 && ex == 0);
  nl_pushrange(L, 0, 2.5, 1);
  CHECK(nl_torange(L, -1, &a, &b, &ex) == 1);
  CHECK(a == 0 && b == 2.5 && ex == 1);
  CHECK_STR(nl_typeof(L, -1), "Table");  /* The fake gives ranges no name. */
  lua_newtable(L);
  CHECK(nl_torange(L, -1, &a, &b, &ex) == 0);
  lua_pushnumber(L, 3);
  CHECK(nl_torange(L, -1, NULL, NULL, NULL) == 0);
  CHECK(lua_gettop(L) == 4);
  lua_close(L);
}

static int checks_unit(lua_State *L)
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

static void test_checkisa_typeerror_tolstring(void)
{
  lua_State *L = newstate();
  size_t len = 0;
  luaL_newmetatable(L, "Unit");
  nl_settype(L, -1, "Unit", 0);
  lua_pushcfunction(L, checks_unit);
  lua_newuserdata(L, 4);
  lua_pushvalue(L, 1);
  lua_setmetatable(L, -2);
  CHECK(nl_pcall(L, 1, 1) == 0);
  CHECK_STR(lua_tostring(L, -1), "accepted");
  lua_pop(L, 1);
  lua_pushcfunction(L, checks_unit);
  lua_pushnumber(L, 5);
  CHECK(nl_pcall(L, 1, 1) != 0);
  CHECK_STR(nl_typeof(L, -1), "TypeError");
  lua_getfield(L, -1, "message");
  CHECK_CONTAINS(lua_tostring(L, -1), "expected Unit, got Number");
  lua_pop(L, 2);
  lua_pushcfunction(L, typeerror_fn);
  lua_pushboolean(L, 1);
  CHECK(nl_pcall(L, 1, 0) != 0);
  CHECK_STR(nl_typeof(L, -1), "TypeError");
  lua_getfield(L, -1, "message");
  CHECK_CONTAINS(lua_tostring(L, -1), "expected Widget, got Bool");
  lua_pop(L, 2);
  /* nlL_tolstring pushes and returns. */
  lua_pushnumber(L, 42);
  CHECK_STR(nlL_tolstring(L, -1, &len), "42");
  CHECK(len == 2 && lua_gettop(L) == 3);
  lua_close(L);
}

/* -- Exceptions ----------------------------------------------------------- */

static int raises_string(lua_State *L)
{
  return luaL_error(L, "plain lua error");
}

static int nl_raises_string(lua_State *L)
{
  lua_pushliteral(L, "raised string");
  return nl_raise(L);
}

static int raises_typed(lua_State *L)
{
  return nlL_raise(L, "TypeError", "amount must not be negative, got %d", -3);
}

static int raises_untyped(lua_State *L)
{
  return nlL_raise(L, NULL, "plain %s", "exception");
}

static int returns_two(lua_State *L)
{
  lua_pushnumber(L, 1);
  lua_pushnumber(L, 2);
  return 2;
}

static void test_pcall_and_raise(void)
{
  lua_State *L = newstate();
  lua_pushcfunction(L, returns_two);
  CHECK(nl_pcall(L, 0, LUA_MULTRET) == 0);
  CHECK(lua_gettop(L) == 2 && lua_tonumber(L, 2) == 2);
  lua_settop(L, 0);
  /* A non-exception error is wrapped in LuaError, with trace and value. */
  lua_pushcfunction(L, raises_string);
  CHECK(nl_pcall(L, 0, 0) == LUA_ERRRUN);
  CHECK(lua_gettop(L) == 1);
  CHECK_STR(nl_typeof(L, -1), "LuaError");
  lua_getfield(L, -1, "trace");
  CHECK(lua_isstring(L, -1));
  lua_getfield(L, -2, "value");
  CHECK_CONTAINS(lua_tostring(L, -1), "plain lua error");
  lua_settop(L, 0);
  /* nl_raise turns a string into an Exception. */
  lua_pushcfunction(L, nl_raises_string);
  CHECK(nl_pcall(L, 0, 0) != 0);
  CHECK_STR(nl_typeof(L, -1), "Exception");
  lua_getfield(L, -1, "message");
  CHECK_STR(lua_tostring(L, -1), "raised string");
  lua_settop(L, 0);
  /* nlL_raise with and without a type. */
  lua_pushcfunction(L, raises_typed);
  CHECK(nl_pcall(L, 0, 0) != 0);
  CHECK_STR(nl_typeof(L, -1), "TypeError");
  lua_getfield(L, -1, "message");
  CHECK_STR(lua_tostring(L, -1), "amount must not be negative, got -3");
  lua_settop(L, 0);
  lua_pushcfunction(L, raises_untyped);
  CHECK(nl_pcall(L, 0, 0) != 0);
  CHECK_STR(nl_typeof(L, -1), "Exception");
  nl_gettype(L, "Exception");
  CHECK(nl_isa(L, -2, -1));
  lua_settop(L, 0);
  /* An exception keeps its identity through Lua's pcall/error and nl_pcall. */
  dolua(L, "kept = __fake_rt.exception('TypeError', 'same one')");
  CHECK(luaL_loadstring(L, "error(kept)") == 0);
  CHECK(nl_pcall(L, 0, 0) != 0);
  lua_getglobal(L, "kept");
  CHECK(lua_rawequal(L, -1, -2));
  lua_settop(L, 0);
  /* Lua's pcall still sees the raw value. */
  CHECK_LUA(L, "select(2, pcall(error, 'raw')) == 'raw'");
  lua_close(L);
}

static int raises_udata(lua_State *L)
{
  int *p = (int *)lua_newuserdata(L, sizeof(int));
  *p = 7;
  luaL_getmetatable(L, "ForeignError");
  lua_setmetatable(L, -2);
  return lua_error(L);
}

static void test_translator(void)
{
  lua_State *L = newstate();
  luaL_newmetatable(L, "ForeignError");
  lua_pop(L, 1);
  dolua(L, "function translate(e) "
	   "  if type(e) == 'userdata' and getmetatable(e) == debug.getregistry().ForeignError then "
	   "    return __fake_rt.exception('TypeError', 'translated') end "
	   "end");
  lua_getglobal(L, "translate");
  nl_settranslator(L);
  CHECK(lua_gettop(L) == 0);
  lua_pushcfunction(L, raises_udata);
  CHECK(nl_pcall(L, 0, 0) != 0);
  CHECK_STR(nl_typeof(L, -1), "TypeError");
  lua_getfield(L, -1, "message");
  CHECK_STR(lua_tostring(L, -1), "translated");
  lua_settop(L, 0);
  /* nil from the translator: the default wrapping. */
  lua_pushcfunction(L, raises_string);
  CHECK(nl_pcall(L, 0, 0) != 0);
  CHECK_STR(nl_typeof(L, -1), "LuaError");
  lua_close(L);
}

/* An allocator that fails on demand, for LUA_ERRMEM. It wraps the state's
** own allocator: memory from realloc can lie above the 47 bits of address a
** GC64 state needs, as on arm64 Linux. */
static int alloc_fail;
static lua_Alloc base_alloc;
static void *base_ud;
static void *limited_alloc(void *ud, void *ptr, size_t osize, size_t nsize)
{
  (void)ud;
  if (alloc_fail && nsize > osize) return NULL;
  return base_alloc(base_ud, ptr, osize, nsize);
}

static int allocates(lua_State *L)
{
  alloc_fail = 1;
  lua_newtable(L);
  lua_createtable(L, 100000, 0);
  return 0;
}

static void test_pcall_errmem(void)
{
  lua_State *L = luaL_newstate();
  CHECK(L != NULL);
  if (!L) return;
  base_alloc = lua_getallocf(L, &base_ud);
  lua_setallocf(L, limited_alloc, NULL);
  luaL_openlibs(L);
  lua_pushcfunction(L, allocates);
  CHECK(nl_pcall(L, 0, 0) == LUA_ERRMEM);
  alloc_fail = 0;
  CHECK(lua_type(L, -1) == LUA_TSTRING);
  CHECK_CONTAINS(lua_tostring(L, -1), "not enough memory");
  lua_setallocf(L, base_alloc, base_ud);  /* lua_close releases the state's own allocator */
  lua_close(L);
}

/* An allocator that keeps the true size of each block in a header, fails
** the failat-th growing allocation, and counts the bytes it holds. It
** draws on the allocator of another state (base_alloc), for the same
** reason as limited_alloc.
*/
typedef struct SizedAlloc { long n, failat; size_t live; } SizedAlloc;
#define SIZED_HDR	16

static void *sized_alloc(void *ud, void *ptr, size_t osize, size_t nsize)
{
  SizedAlloc *sa = (SizedAlloc *)ud;
  char *b = ptr ? (char *)ptr - SIZED_HDR : NULL;
  size_t real = b ? *(size_t *)b : 0;
  (void)osize;
  if (nsize == 0) {
    if (b) { sa->live -= real; base_alloc(base_ud, b, real + SIZED_HDR, 0); }
    return NULL;
  }
  if (nsize > real && ++sa->n == sa->failat) return NULL;
  b = (char *)base_alloc(base_ud, b, b ? real + SIZED_HDR : 0, nsize + SIZED_HDR);
  if (!b) return NULL;
  *(size_t *)b = nsize;
  sa->live = sa->live - real + nsize;
  return b + SIZED_HDR;
}

static int dumpbuf_writer(lua_State *L, const void *p, size_t sz, void *ud)
{
  luaL_Buffer *b = (luaL_Buffer *)ud;
  (void)L;
  luaL_addlstring(b, (const char *)p, sz);
  return 0;
}

/* Closures are made after their upvalues: when making an upvalue runs out
** of memory, no closure that counts none of them is left to the collector,
** which would free it with a smaller size than it has. Each growing
** allocation fails in turn while closures with upvalues are made, by FNEW
** and by lua_load of bytecode whose main function has upvalues; the GC's
** count of bytes must then match what the allocator holds. (An assertion
** build also checks it when the state closes.)
*/
static void test_closure_oom(void)
{
  static const char code[] =
    "local a, b, r = {}, {}, {}\n"
    "for i = 1, 20 do local c = i; r[i] = function() return a, b, c end end\n"
    "return r[20]";
  lua_State *L0 = luaL_newstate(), *L;
  const char *bc;
  size_t bclen;
  int load, failed = 0;
  CHECK(L0 != NULL);
  if (!L0) return;
  base_alloc = lua_getallocf(L0, &base_ud);
  {
    luaL_Buffer b;
    CHECK(luaL_loadstring(L0, code) == 0 && lua_pcall(L0, 0, 1, 0) == 0);
    luaL_buffinit(L0, &b);
    CHECK(lua_dump(L0, dumpbuf_writer, &b) == 0);
    luaL_pushresult(&b);
    bc = lua_tolstring(L0, -1, &bclen);
  }
  for (load = 0; load <= 1; load++) {
    long k;
    for (k = 1; k < 100000; k++) {
      SizedAlloc sa;
      int st;
      size_t total;
      sa.n = 0; sa.failat = 0; sa.live = 0;
      L = lua_newstate(sized_alloc, &sa);
      CHECK(L != NULL);
      if (!L) break;
      luaL_openlibs(L);
      if (!load) CHECK(luaL_loadstring(L, code) == 0);
      sa.failat = sa.n + k;
      st = load ? luaL_loadbuffer(L, bc, bclen, "=bc") : lua_pcall(L, 0, 0, 0);
      sa.failat = 0;
      lua_settop(L, 0);
      lua_gc(L, LUA_GCCOLLECT, 0);
      total = (size_t)lua_gc(L, LUA_GCCOUNT, 0) * 1024 +
	      (size_t)lua_gc(L, LUA_GCCOUNTB, 0);
      if (total != sa.live) {
	fprintf(stderr, "%s, allocation %ld failing: GC count %lu, allocator %lu\n",
		load ? "lua_load" : "FNEW", k, (unsigned long)total,
		(unsigned long)sa.live);
	CHECK(total == sa.live);
	lua_close(L);
	break;
      }
      lua_close(L);
      CHECK(sa.live == 0);
      if (st == 0) break;  /* Nothing failed: every point was tried. */
      CHECK(st == LUA_ERRMEM);
      failed++;
    }
  }
  printf("# closures with upvalues, each allocation failing in turn: %d failed cleanly\n", failed);
  CHECK(failed > 0);
  lua_close(L0);
}

/* -- Events (C side; tests/vm/test_events.lua has the Lua side) ---------- */

static int handler_a(lua_State *L)
{
  lua_getglobal(L, "log");
  lua_pushfstring(L, "%sa(%s)", lua_tostring(L, -1), luaL_optstring(L, 1, ""));
  lua_setglobal(L, "log");
  return 0;
}

static int handler_returns(lua_State *L)
{
  lua_pushliteral(L, "decided");
  lua_pushnumber(L, lua_tonumber(L, 2) * 2);
  return 2;
}

static int handler_replaced(lua_State *L)
{
  lua_getglobal(L, "log");
  lua_pushfstring(L, "%sR", lua_tostring(L, -1));
  lua_setglobal(L, "log");
  return 0;
}

static int handler_fails(lua_State *L)
{
  return luaL_error(L, "handler failed");
}

static void test_events_c(void)
{
  lua_State *L = newstate();
  dolua(L, "log = ''");
  lua_pushcfunction(L, handler_a);
  nl_onevent(L, "Game.Tick", "a");
  dolua(L, "event.on_run('Game.Tick', 'lua', function(x) log = log .. 'l(' .. tostring(x) .. ')' end)");
  lua_pushcfunction(L, handler_returns);
  nl_onevent(L, "Game.Tick", "ret");
  CHECK(lua_gettop(L) == 0);
  lua_pushliteral(L, "x");
  lua_pushnumber(L, 21);
  nl_runevent(L, "Game.Tick", 2, LUA_MULTRET);
  CHECK(lua_gettop(L) == 2);
  CHECK_STR(lua_tostring(L, 1), "decided");
  CHECK(lua_tonumber(L, 2) == 42);
  lua_settop(L, 0);
  CHECK_LUA(L, "log == 'a(x)l(x)'");
  /* nresults adjusts. */
  dolua(L, "log = ''");
  nl_runevent(L, "Game.Tick", 0, 3);
  CHECK(lua_gettop(L) == 3 && lua_isnil(L, 3));
  lua_settop(L, 0);
  /* Replacing keeps the place. */
  dolua(L, "log = ''");
  lua_pushcfunction(L, handler_replaced);
  nl_onevent(L, "Game.Tick", "a");
  nl_runevent(L, "Game.Tick", 0, 0);
  CHECK_LUA(L, "log == 'Rl(nil)'");
  /* Removal, from C and from Lua; removing what is not there is nothing. */
  nl_offevent(L, "Game.Tick", "ret");
  nl_offevent(L, "Game.Tick", "never");
  nl_offevent(L, "No.Such.Event", "x");
  dolua(L, "log = ''");
  nl_runevent(L, "Game.Tick", 0, LUA_MULTRET);
  CHECK(lua_gettop(L) == 0);
  CHECK_LUA(L, "log == 'Rl(nil)'");
  dolua(L, "event.remove('Game.Tick', 'lua')");
  dolua(L, "log = ''");
  nl_runevent(L, "Game.Tick", 0, 0);
  CHECK_LUA(L, "log == 'R'");
  /* An event without handlers returns nothing. */
  nl_runevent(L, "Nobody.Listens", 0, LUA_MULTRET);
  CHECK(lua_gettop(L) == 0);
  /* nl_prunevent: status and the exception. */
  lua_pushcfunction(L, handler_fails);
  nl_onevent(L, "Fails", "f");
  lua_pushnumber(L, 1);
  CHECK(nl_prunevent(L, "Fails", 1, 0) == LUA_ERRRUN);
  CHECK(lua_gettop(L) == 1);
  CHECK_STR(nl_typeof(L, -1), "LuaError");
  lua_getfield(L, -1, "trace");
  CHECK(lua_isstring(L, -1));
  lua_settop(L, 0);
  lua_pushnumber(L, 5);
  CHECK(nl_prunevent(L, "Game.Tick", 1, 1) == 0);
  CHECK(lua_gettop(L) == 1 && lua_isnil(L, 1));
  lua_settop(L, 0);
  lua_close(L);
}

/* -- Tasks ---------------------------------------------------------------- */

static void test_tasks(void)
{
  lua_State *L = newstate();
  int t;
  nl_newtask(L);
  t = lua_gettop(L);
  CHECK(nl_taskstate(L, t) == NL_TASK_PENDING);
  CHECK(lua_gettop(L) == t);
  /* on_done runs inside nl_resolve. */
  lua_pushvalue(L, t);
  lua_setglobal(L, "task");
  dolua(L, "seen = nil; task:on_done(function(v) seen = v end)");
  lua_pushliteral(L, "the value");
  nl_resolve(L, t);
  CHECK(lua_gettop(L) == t);
  CHECK_LUA(L, "seen == 'the value'");
  CHECK(nl_taskstate(L, t) == NL_TASK_DONE);
  CHECK_STR(lua_tostring(L, -1), "the value");
  lua_pop(L, 1);
  /* Finishing twice does nothing the second time. */
  lua_pushliteral(L, "other");
  nl_resolve(L, -2);
  lua_pushliteral(L, "late failure");
  nl_reject(L, t);
  CHECK(nl_taskstate(L, t) == NL_TASK_DONE);
  CHECK_STR(lua_tostring(L, -1), "the value");
  lua_settop(L, 0);
  /* Reject with raise's conversion; on_error runs inside nl_reject. */
  nl_newtask(L);
  lua_pushvalue(L, -1);
  lua_setglobal(L, "task2");
  dolua(L, "err = nil; task2:on_error(function(e) err = e end)");
  lua_pushliteral(L, "went wrong");
  nl_reject(L, 1);
  CHECK_LUA(L, "err ~= nil and err.message == 'went wrong'");
  CHECK(nl_taskstate(L, 1) == NL_TASK_FAILED);
  CHECK_STR(nl_typeof(L, -1), "Exception");
  lua_settop(L, 0);
  /* Not a task. */
  lua_pushnumber(L, 1);
  CHECK(nl_taskstate(L, 1) == -1);
  CHECK(lua_gettop(L) == 1);
  lua_settop(L, 0);
  lua_close(L);
}

static void test_unhandled(void)
{
  lua_State *L = newstate();
  dolua(L, "caught = {}");
  CHECK(luaL_loadstring(L, "local e = ... ; caught[#caught + 1] = e") == 0);
  nl_setunhandled(L);
  CHECK(lua_gettop(L) == 0);
  nl_newtask(L);
  lua_pushliteral(L, "nobody waits");
  nl_reject(L, 1);
  CHECK_LUA(L, "#caught == 1 and caught[1].message == 'nobody waits'");
  lua_close(L);
}

typedef struct TimerCall {
  int calls;
  double seconds;
  lua_State *L;
  nl_Callback wake;
  void *arg;
} TimerCall;

static void timer(void *data, lua_State *L, double seconds, nl_Callback wake,
		  void *arg)
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
  memset(&tc, 0, sizeof(tc));
  /* Without a timer, sleeping raises. */
  CHECK_LUA(L, "not pcall(__fake_rt.sleep, 1)");
  nl_settimer(L, timer, &tc);
  /* Sleep inside a coroutine: the timer still gets the main thread. */
  dolua(L, "woke = nil; co = coroutine.create(function() "
	   "  __fake_rt.sleep(2.5):on_done(function(v) woke = v end) end); "
	   "assert(coroutine.resume(co))");
  CHECK(tc.calls == 1 && tc.seconds == 2.5);
  CHECK(tc.L == L);
  CHECK_LUA(L, "woke == nil");
  tc.wake(tc.L, tc.arg);  /* Directly from the host's loop. */
  CHECK_LUA(L, "woke == 2.5");
  /* Through nl_post, as from a timer thread. */
  dolua(L, "woke = nil; __fake_rt.sleep(1):on_done(function(v) woke = v end)");
  CHECK(tc.calls == 2);
  CHECK(nl_post(L, tc.wake, tc.arg) == 1);
  CHECK_LUA(L, "woke == nil");
  CHECK(nl_pump(L) == 1);
  CHECK_LUA(L, "woke == 1");
  /* A wake still posted when the state closes is only freed. */
  dolua(L, "__fake_rt.sleep(3)");
  CHECK(tc.calls == 3);
  CHECK(nl_post(L, tc.wake, tc.arg) == 1);
  nl_settimer(L, NULL, NULL);
  CHECK_LUA(L, "not pcall(__fake_rt.sleep, 1)");
  lua_close(L);
}

/* -- Inbox, single thread (tests/vm/test_threads.c has the threads) ------ */

static char post_log[256];
static int post_null_calls;

static void post_cb(lua_State *L, void *data)
{
  size_t n = strlen(post_log);
  if (!L) { post_null_calls++; return; }
  if (n + 2 < sizeof(post_log)) {
    post_log[n] = *(const char *)data;
    post_log[n+1] = '\0';
  }
  CHECK(lua_gettop(L) == 0);  /* A fresh frame. */
  lua_pushliteral(L, "left on the stack");
}

static void post_raises(lua_State *L, void *data)
{
  (void)data;
  if (L) luaL_error(L, "callback raised");
}

static lua_State *repost_L;
static void post_reposts(lua_State *L, void *data)
{
  if (L) nl_post(repost_L, post_cb, data);
}

static int wakeups;
static void wakeup(void *data)
{
  CHECK(data == (void *)&wakeups);
  wakeups++;
}

static void test_post_and_pump(void)
{
  lua_State *L = newstate();
  static const char a = 'a', b = 'b', c = 'c', d = 'd';
  post_log[0] = '\0';
  CHECK(nl_pump(L) == 0);
  nl_setwakeup(L, wakeup, &wakeups);
  wakeups = 0;
  CHECK(nl_post(L, post_cb, (void *)&a) == 1);
  CHECK(wakeups == 1);
  CHECK(nl_post(L, post_cb, (void *)&b) == 1);
  CHECK(wakeups == 1);  /* Only when the inbox stops being empty. */
  CHECK(nl_post(L, post_raises, NULL) == 1);
  CHECK(nl_post(L, post_cb, (void *)&c) == 1);
  CHECK(nl_post(NULL, post_cb, NULL) == 0);
  CHECK(nl_post(L, NULL, NULL) == 0);
  /* Errors go to the unhandled hook; the pump goes on. */
  dolua(L, "caught = {}");
  CHECK(luaL_loadstring(L, "caught[#caught + 1] = ...") == 0);
  nl_setunhandled(L);
  CHECK(nl_pump(L) == 4);
  CHECK_STR(post_log, "abc");
  CHECK(lua_gettop(L) == 0);
  CHECK_LUA(L, "#caught == 1 and caught[1].message:find('callback raised') ~= nil");
  CHECK_LUA(L, "caught[1].trace ~= nil");
  /* What a callback posts waits for the next pump. */
  repost_L = L;
  CHECK(nl_post(L, post_reposts, (void *)&d) == 1);
  CHECK(wakeups == 2);
  CHECK(nl_pump(L) == 1);
  CHECK_STR(post_log, "abc");
  CHECK(nl_pump(L) == 1);
  CHECK_STR(post_log, "abcd");
  CHECK(nl_post(L, post_cb, (void *)&a) == 1);
  nl_setwakeup(L, NULL, NULL);
  /* Still queued at lua_close: called once with L == NULL. */
  post_null_calls = 0;
  CHECK(nl_post(L, post_cb, (void *)&b) == 1);
  lua_close(L);
  CHECK(post_null_calls == 2);
  /* abc, the repost, what it posted (the inbox was empty again), a. */
  CHECK(wakeups == 4);
}


/* -- Robustness ----------------------------------------------------------- */

/* A load error that is not a string becomes a message without using
** a string after it could be collected (run under ASan by run.sh).
*/
static void test_load_error_not_a_string(void)
{
  lua_State *L = newstate();
  int i;
  dolua(L, "saved_load = __fake_rt.load; "
	   "__fake_rt.load = function() return nil, setmetatable({}, {__tostring = function() "
	   "  return 'fresh message ' .. string.rep('m', 64) .. tostring(math.random()) end}) end; "
	   "collectgarbage('setpause', 0); collectgarbage('setstepmul', 100000)");
  for (i = 0; i < 50; i++) {
    CHECK(nlL_loadstring(L, "return 1") == LUA_ERRSYNTAX);
    CHECK_CONTAINS(lua_tostring(L, -1), "fresh message mmmm");
    lua_pop(L, 1);
  }
  /* A raised error value that is not a string, through nl_loadfile too. */
  dolua(L, "__fake_rt.load = function() error(setmetatable({}, {__tostring = function() "
	   "  return 'raised ' .. tostring(math.random()) end})) end");
  writefile("tm.nl", "return 1");
  for (i = 0; i < 50; i++) {
    CHECK(nl_loadfile(L, "tm.nl") == LUA_ERRSYNTAX);
    CHECK_CONTAINS(lua_tostring(L, -1), "raised ");
    lua_pop(L, 1);
  }
  dolua(L, "__fake_rt.load = saved_load; collectgarbage('setpause', 200)");
  lua_close(L);
}

/* Files loaded by name get the canonical name as chunk name. */
static void test_chunk_names_of_files(void)
{
  lua_State *L = newstate();
  mkdir("cn", 0777);
  writefile("cn/src.nl", "return debug.getinfo(1, 'S').source");
  CHECK(luaL_loadfile(L, "cn/src.nl") == 0 && lua_pcall(L, 0, 1, 0) == 0);
  CHECK_STR(lua_tostring(L, -1), "@cn/src.nl");
  lua_pop(L, 1);
  CHECK(luaL_loadfile(L, "./cn/../cn/src.nl") == 0 && lua_pcall(L, 0, 1, 0) == 0);
  CHECK_STR(lua_tostring(L, -1), "@cn/src.nl");
  lua_pop(L, 1);
  CHECK(nl_loadfile(L, "cn//src.nl") == 0 && lua_pcall(L, 0, 1, 0) == 0);
  CHECK_STR(lua_tostring(L, -1), "@cn/src.nl");
  lua_pop(L, 1);
  CHECK_LUA(L, "include('./cn/src.nl') == '@cn/src.nl'");
  /* The same name as resolve gives. */
  CHECK_LUA(L, "__fake_rt.host.resolve(nil, 'cn/src') == 'cn/src.nl'");
  /* NULL is refused. */
  CHECK(nl_loadfile(L, NULL) == LUA_ERRFILE);
  lua_pop(L, 1);
  lua_close(L);
}

/* A chunk name that is the source shows as [string "..."]. */
static void test_source_string_names(void)
{
  lua_State *L = newstate();
  const char *two = "return 1\n--@error bad-thing it went wrong";
  const char *longsrc = "--@error e1 first error in a source string that is much longer than sixty characters";
  CHECK(nlL_loadstring(L, two) == LUA_ERRSYNTAX);
  CHECK_STR(lua_tostring(L, -1), "[string \"return 1...\"]:2:1: it went wrong");
  lua_pop(L, 1);
  CHECK(nlL_loadstring(L, longsrc) == LUA_ERRSYNTAX);
  CHECK_STR(lua_tostring(L, -1),
	    "[string \"--@error e1 first error in a source string th...\"]:1:1: "
	    "first error in a source string that is much longer than sixty characters");
  lua_pop(L, 1);
  /* Lua shows the same name in debug information. */
  CHECK(nlL_loadstring(L, "return debug.getinfo(1, 'S').short_src") == 0);
  CHECK(lua_pcall(L, 0, 1, 0) == 0);
  CHECK_STR(lua_tostring(L, -1), "[string \"return debug.getinfo(1, 'S').short_src\"]");
  lua_pop(L, 1);
  lua_close(L);
}

/* What a NULL callback posts during lua_close is called too. */
static lua_State *closing_L;
static int close_null_calls, close_reposts;
static void close_second(lua_State *L, void *data)
{
  (void)data;
  if (!L) close_null_calls++;
}
static void close_first(lua_State *L, void *data)
{
  (void)data;
  if (!L) {
    close_null_calls++;
    close_reposts += nl_post(closing_L, close_second, NULL);
  }
}

static void test_post_during_close(void)
{
  lua_State *L = newstate();
  closing_L = L;
  close_null_calls = close_reposts = 0;
  CHECK(nl_post(L, close_first, NULL) == 1);
  lua_close(L);
  CHECK(close_reposts == 1);
  CHECK(close_null_calls == 2);
}

/* A damaged runtime or registry gives errors, never a crash
** or a raise from a function that raises nothing.
*/
static int panicked;
static int at_panic(lua_State *L)
{
  (void)L;
  panicked++;
  fprintf(stderr, "PANIC: %s\n", lua_tostring(L, -1));
  exit(3);
}

static void test_damaged_runtime(void)
{
  lua_State *L = newstate();
  lua_Number a, b;
  int ex;
  lua_atpanic(L, at_panic);
  /* rt.handler gone: nl_pcall is a plain protected call. */
  dolua(L, "saved = {}; for k, v in pairs(__fake_rt) do saved[k] = v end; __fake_rt.handler = nil");
  lua_pushcfunction(L, raises_string);
  CHECK(nl_pcall(L, 0, 0) == LUA_ERRRUN);
  CHECK(lua_type(L, -1) == LUA_TSTRING);
  lua_pop(L, 1);
  /* rt.typeof returning a number: no name, and no dangling pointer. */
  dolua(L, "__fake_rt.typeof = function() return 12345.5 end");
  lua_pushnil(L);
  CHECK(nl_typeof(L, -1) == NULL);
  lua_pop(L, 1);
  /* rt functions that raise or are missing: '-' functions return failure. */
  dolua(L, "__fake_rt.typeof = nil; __fake_rt.isa = function() error('broken') end; "
	   "__fake_rt.gettype = 7; __fake_rt.torange = nil; __fake_rt.taskstate = nil");
  lua_pushnil(L);
  CHECK(nl_typeof(L, -1) == NULL);
  CHECK(nl_isa(L, -1, -1) == 0);
  nl_gettype(L, "Exception");
  CHECK(lua_isnil(L, -1));
  lua_pop(L, 1);
  CHECK(nl_torange(L, -1, &a, &b, &ex) == 0);
  CHECK(nl_taskstate(L, -1) == -1);
  CHECK(lua_gettop(L) == 1);
  lua_pop(L, 1);
  lua_pushnil(L);
  nl_settranslator(L);
  lua_pushnil(L);
  nl_setunhandled(L);
  CHECK(lua_gettop(L) == 0);
  /* The registry slot of rt replaced by a number. */
  dolua(L, "for k, v in pairs(saved) do __fake_rt[k] = v end; "
	   "local reg = debug.getregistry(); "
	   "for k, v in pairs(reg) do if v == __fake_rt then reg[k] = 42 end end");
  lua_pushnumber(L, 1);
  CHECK(nl_typeof(L, -1) == NULL);
  CHECK(nl_isa(L, -1, -1) == 0);
  lua_pushcfunction(L, raises_string);
  CHECK(nl_pcall(L, 0, 0) == LUA_ERRRUN);
  lua_settop(L, 0);
  CHECK(nl_pump(L) == 0);
  CHECK(nlL_loadstring(L, "return 1") != 0);  /* rt.load unreachable. */
  lua_settop(L, 0);
  CHECK(panicked == 0);
  lua_close(L);
}

/* The internal C functions in the registry, called from Lua with a
** userdata or nothing, raise errors; guards refuse a foreign userdata.
*/
static void test_internal_functions_from_lua(void)
{
  lua_State *L = newstate();
  lua_atpanic(L, at_panic);
  lua_pushnumber(L, 1);
  nl_typeof(L, -1);  /* The trampoline exists. */
  lua_pop(L, 1);
  CHECK(nlL_loadstring(L, "return 1") == 0);  /* So does the guard metatable. */
  lua_pop(L, 1);
  CHECK_LUA(L, "(function() "
	       "  for k, v in pairs(debug.getregistry()) do "
	       "    if type(v) == 'function' then pcall(v, io.stdout); pcall(v) end "
	       /* The guard metatable: a numeric reference holding only __gc.
	       ** (Stock metatables are left alone: giving a stock __gc a
	       ** foreign userdata crashes stock LuaJIT as well.) */
	       "    if type(k) == 'number' and type(v) == 'table' "
	       "       and type(rawget(v, '__gc')) == 'function' and next(v, next(v)) == nil then "
	       "      local ud = newproxy(false); debug.setmetatable(ud, v) "
	       "      pcall(v.__gc, io.stdout); pcall(v.__gc, ud) "
	       "    end "
	       "  end "
	       "  collectgarbage(); collectgarbage() "
	       "  return true end)()");
  /* Everything still works. */
  lua_pushnumber(L, 1);
  CHECK_STR(nl_typeof(L, -1), "Number");
  lua_pop(L, 1);
  CHECK(nlL_dostring(L, "return 1") == 0);
  lua_settop(L, 0);
  /* A damaged guard metatable slot is replaced. */
  CHECK_LUA(L, "(function() local reg = debug.getregistry() "
	       "  for k, v in pairs(reg) do "
	       "    if type(k) == 'number' and type(v) == 'table' and type(rawget(v, '__gc')) == 'function' then reg[k] = 5 end end "
	       "  return true end)()");
  CHECK(nlL_dostring(L, "return 2") == 0);
  CHECK(lua_tonumber(L, -1) == 2);
  lua_settop(L, 0);
  CHECK(panicked == 0);
  lua_close(L);
}

/* Runtime queue: nl_pump drives rt.pump. */
static void test_pump_drives_runtime_queue(void)
{
  lua_State *L = newstate();
  dolua(L, "ran = 0; __fake_rt.post(function() ran = ran + 1 end)");
  CHECK(nl_pump(L) == 1);
  CHECK_LUA(L, "ran == 1");
  CHECK(nl_pump(L) == 0);
  /* A failure in the runtime's queue goes to the unhandled hook. */
  dolua(L, "caught = {}; __fake_rt.setunhandled(function(e) caught[#caught + 1] = e end); "
	   "__fake_rt.post(function() error('queued failure') end)");
  CHECK(nl_pump(L) == 1);
  CHECK_LUA(L, "#caught == 1 and tostring(caught[1].message):find('queued failure') ~= nil");
  lua_close(L);
}

/* NL not open: every function behaves as documented. */
static void test_not_open(void)
{
  lua_State *L = luaL_newstate();
  lua_Number a, b;
  int ex;
  lua_pushcfunction(L, luaopen_base); lua_call(L, 0, 0);
  writefile("no.nl", "return 1");
  CHECK(nl_loadfile(L, "no.nl") == LUA_ERRFILE);
  CHECK_CONTAINS(lua_tostring(L, -1), "NL is not open");
  lua_pop(L, 1);
  CHECK(luaL_loadfile(L, "no.nl") == LUA_ERRFILE);
  lua_pop(L, 1);
  CHECK(nlL_loadstring(L, "return 1") == LUA_ERRFILE);
  lua_pop(L, 1);
  lua_pushnil(L);
  CHECK(nl_typeof(L, -1) == NULL);
  CHECK(nl_isa(L, -1, -1) == 0);
  CHECK(nl_torange(L, -1, &a, &b, &ex) == 0);
  CHECK(nl_taskstate(L, -1) == -1);
  nl_gettype(L, "Exception");
  CHECK(lua_isnil(L, -1));
  lua_settop(L, 0);
  lua_pushcfunction(L, raises_string);
  CHECK(nl_pcall(L, 0, 0) == LUA_ERRRUN);
  CHECK(lua_type(L, -1) == LUA_TSTRING);
  lua_settop(L, 0);
  CHECK(nl_pump(L) == 0);
  nl_offevent(L, "E", "h");
  CHECK(lua_gettop(L) == 0);
  lua_close(L);
}

static void test_version_symbol(void)
{
  NL_VERSION_SYM();
  CHECK(NL_VERSION_NUM == 100);
  CHECK_STR(NL_VERSION, "NL 0.1");
}

int main(int argc, char **argv)
{
  if (argc > 1 && chdir(argv[1]) != 0) { perror(argv[1]); return 2; }
  RUN_TEST(test_version_symbol);
  RUN_TEST(test_openlibs_opens_nl);
  RUN_TEST(test_luaopen_nl_by_hand);
  RUN_TEST(test_options);
  RUN_TEST(test_glua);
  RUN_TEST(test_openlibs_option);
  RUN_TEST(test_glua_metamethod_rules);
  RUN_TEST(test_loadbuffer);
  RUN_TEST(test_compile_errors);
  RUN_TEST(test_choice_by_extension);
  RUN_TEST(test_import_and_search_order);
  RUN_TEST(test_loader);
  RUN_TEST(test_types);
  RUN_TEST(test_settype_twice_raises);
  RUN_TEST(test_ranges);
  RUN_TEST(test_checkisa_typeerror_tolstring);
  RUN_TEST(test_pcall_and_raise);
  RUN_TEST(test_translator);
  RUN_TEST(test_pcall_errmem);
  RUN_TEST(test_closure_oom);
  RUN_TEST(test_events_c);
  RUN_TEST(test_tasks);
  RUN_TEST(test_unhandled);
  RUN_TEST(test_timer);
  RUN_TEST(test_post_and_pump);
  RUN_TEST(test_load_error_not_a_string);
  RUN_TEST(test_chunk_names_of_files);
  RUN_TEST(test_source_string_names);
  RUN_TEST(test_post_during_close);
  RUN_TEST(test_damaged_runtime);
  RUN_TEST(test_internal_functions_from_lua);
  RUN_TEST(test_pump_drives_runtime_queue);
  RUN_TEST(test_not_open);
  return check_summary("test_capi");
}
