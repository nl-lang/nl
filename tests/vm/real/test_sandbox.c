/*
** The sandbox profile (NL_OPT_SANDBOX, nl_sandboxenv) against the real
** runtime and the real compiler.
** Built and run by tests/vm/real/run.sh, with a scratch directory as
** argv[1]; the test works inside it. The conformance programs that run
** under the sandbox are in tests/vm/real/run.sh (nlrun --sandbox).
*/

#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <unistd.h>
#include <sys/stat.h>

#include "lua.h"
#include "lauxlib.h"
#include "lualib.h"
#include "nl.h"

#include "support/check.h"

#define NOBC "bytecode loading is off in the sandbox"

static int openlibs(lua_State *L)
{
  luaL_openlibs(L);
  return 0;
}

static lua_State *newstate(int sandbox)
{
  lua_State *L = luaL_newstate();
  lua_pushcfunction(L, openlibs);
  if (lua_pcall(L, 0, 0, 0) != 0) {
    fprintf(stderr, "luaL_openlibs: %s\n", lua_tostring(L, -1));
    exit(1);
  }
  if (sandbox && !nl_setoption(L, NL_OPT_SANDBOX, 1)) {
    fprintf(stderr, "nl_setoption(NL_OPT_SANDBOX, 1) failed\n");
    exit(1);
  }
  return L;
}

static void writefile(const char *name, const char *text, size_t len)
{
  FILE *fp = fopen(name, "wb");
  if (!fp) { perror(name); exit(2); }
  fwrite(text, 1, len, fp);
  fclose(fp);
}
#define writetext(name, text)	writefile(name, text, strlen(text))

static const char *errtext(lua_State *L)
{
  if (lua_type(L, -1) == LUA_TSTRING) return lua_tostring(L, -1);
  return nlL_tolstring(L, -1, NULL);
}

/* Runs NL source with nl_loadbuffer; leaves the export table (nil on
** failure, which is a failed check).
*/
static int donl(lua_State *L, const char *src, const char *name)
{
  int top = lua_gettop(L);
  int st = nl_loadbuffer(L, src, strlen(src), name);
  if (st == 0) st = nl_pcall(L, 0, 1);
  if (st != 0) {
    fprintf(stderr, "donl(%s): %s\n", name, errtext(L));
    lua_settop(L, top);
    lua_pushnil(L);
    check_failed_total++; check_test_failed = 1;
  }
  return st;
}

/* Runs Lua code in the state's own globals (the host's view). */
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

/* Runs Lua code as a mod: with the sandbox environment, as a host that
** loads Lua mods itself gives it to them.
*/
static int dosandbox(lua_State *L, const char *code)
{
  int top = lua_gettop(L);
  if (luaL_loadstring(L, code)) {
    fprintf(stderr, "dosandbox: %s\n", lua_tostring(L, -1));
    check_failed_total++; check_test_failed = 1;
    lua_settop(L, top);
    return 0;
  }
  nl_sandboxenv(L);
  lua_setfenv(L, -2);
  if (lua_pcall(L, 0, LUA_MULTRET, 0)) {
    fprintf(stderr, "dosandbox: %s\n", errtext(L));
    check_failed_total++; check_test_failed = 1;
    lua_settop(L, top);
    return 0;
  }
  return lua_gettop(L) - top;
}

#define CHECK_IN(L, run, expr) do { \
  int n_ = run(L, "return " expr); \
  check_count++; \
  if (n_ < 1 || !lua_toboolean(L, -n_)) { \
    check_failed_total++; check_test_failed = 1; \
    fprintf(stderr, "%s:%d: %s failed: %s\n", __FILE__, __LINE__, #run, expr); \
  } \
  lua_pop(L, n_); \
} while (0)
#define CHECK_LUA(L, expr)	CHECK_IN(L, dolua, expr)
#define CHECK_SBX(L, expr)	CHECK_IN(L, dosandbox, expr)

/* Field `k` of the table on top as a string ("" when absent). */
static const char *field(lua_State *L, const char *k)
{
  const char *s;
  if (!lua_istable(L, -1)) return "";
  lua_getfield(L, -1, k);
  s = lua_isstring(L, -1) ? lua_tostring(L, -1) : "";
  lua_pop(L, 1);  /* The export table anchors the string. */
  return s;
}
static int fieldbool(lua_State *L, const char *k)
{
  int b;
  if (!lua_istable(L, -1)) return -1;
  lua_getfield(L, -1, k);
  b = lua_isboolean(L, -1) ? lua_toboolean(L, -1) : -1;
  lua_pop(L, 1);
  return b;
}

/* Two bytecode chunks made with the option off: a Lua dump (string.dump)
** and an NL dump (lua_dump of a chunk from the compiler).
*/
static char *luadump, *nldump;
static size_t luadumplen, nldumplen;

typedef struct Buf { char *p; size_t n; } Buf;
static int bufwriter(lua_State *L, const void *p, size_t sz, void *ud)
{
  Buf *b = (Buf *)ud;
  (void)L;
  b->p = (char *)realloc(b->p, b->n + sz);
  memcpy(b->p + b->n, p, sz);
  b->n += sz;
  return 0;
}

static void make_dumps(void)
{
  lua_State *L = newstate(0);
  Buf b = { NULL, 0 };
  size_t len;
  const char *s;
  CHECK(dolua(L, "return string.dump(function() return 'from a lua dump' end)") == 1);
  s = lua_tolstring(L, -1, &len);
  luadump = (char *)malloc(len);
  memcpy(luadump, s, len);
  luadumplen = len;
  lua_pop(L, 1);
  CHECK(nl_loadbuffer(L, "pub const v = 7\n", 16, "=dumped") == 0);
  CHECK(lua_dump(L, bufwriter, &b) == 0);
  nldump = b.p;
  nldumplen = b.n;
  CHECK(luadumplen > 4 && memcmp(luadump, "\033LJ", 3) == 0);
  CHECK(nldumplen > 4 && memcmp(nldump, "\033LJ", 3) == 0);
  /* With the option off, the dumps load where Lua takes bytecode. */
  CHECK(luaL_loadbuffer(L, nldump, nldumplen, "=nldump") == 0);
  CHECK(luaL_loadbuffer(L, luadump, luadumplen, "=luadump") == 0);
  lua_close(L);
}

/* -- The option ----------------------------------------------------------- */

static void test_option(void)
{
  lua_State *L = newstate(0);
  CHECK(nl_getoption(L, NL_OPT_SANDBOX) == 0);
  CHECK(nl_setoption(L, NL_OPT_SANDBOX, 0) == 1);
  CHECK(nl_setoption(L, NL_OPT_SANDBOX, 2) == 0);
  CHECK(nl_setoption(L, NL_OPT_SANDBOX, -1) == 0);
  CHECK(nl_setoption(L, NL_OPT_SANDBOX, 1) == 1);
  CHECK(nl_getoption(L, NL_OPT_SANDBOX) == 1);
  /* One way: the sandbox stays on. */
  CHECK(nl_setoption(L, NL_OPT_SANDBOX, 0) == 0);
  CHECK(nl_getoption(L, NL_OPT_SANDBOX) == 1);
  CHECK(nl_setoption(L, NL_OPT_SANDBOX, 1) == 1);
  lua_close(L);
}

/* NL_OPT_LOCALIZE in the sandbox: the localized
** paths are read from the sandbox environment when the file loads; `io`
** and `os` are not there, so a file that names them in code it never runs
** still loads, and one that runs it fails at the use, as without the
** option; a global alone (2) is the sandbox's.
*/
static void test_localize(void)
{
  int level;
  for (level = 0; level <= 2; level++) {
    lua_State *L = newstate(1);
    const char *src = "pub fn write(s)\n  :io.write(s)\nend\n"
		      "pub fn up(s)\n  :string.upper(s)\nend\n"
		      "pub fn has_print\n  :print != nil\nend\n";
    CHECK(nl_setoption(L, NL_OPT_LOCALIZE, level) == 1);
    CHECK(nl_loadbuffer(L, src, strlen(src), "=localize") == 0);
    CHECK(nl_pcall(L, 0, 1) == 0);
    lua_getfield(L, -1, "up");
    lua_pushliteral(L, "a");
    CHECK(nl_pcall(L, 1, 1) == 0);
    CHECK(strcmp(lua_tostring(L, -1), "A") == 0);
    lua_pop(L, 1);
    lua_getfield(L, -1, "has_print");
    CHECK(nl_pcall(L, 0, 1) == 0);
    CHECK(lua_toboolean(L, -1));
    lua_pop(L, 1);
    lua_getfield(L, -1, "write");
    lua_pushliteral(L, "x");
    CHECK(nl_pcall(L, 1, 1) != 0);
    lua_close(L);
  }
}

/* -- (a) Bytecode loading is off ------------------------------------------ */

static void test_bytecode_host(void)
{
  lua_State *L = newstate(1);
  int st;
  st = nl_loadbuffer(L, nldump, nldumplen, "=nldump");
  CHECK(st == LUA_ERRSYNTAX);
  CHECK_STR(lua_tostring(L, -1), NOBC);
  lua_pop(L, 1);
  st = nl_loadbuffer(L, luadump, luadumplen, "=luadump");
  CHECK(st == LUA_ERRSYNTAX);
  CHECK_STR(lua_tostring(L, -1), NOBC);
  lua_pop(L, 1);
  /* nl_loadfile and luaL_loadfile of a .nl file. */
  writefile("bc.nl", nldump, nldumplen);
  st = nl_loadfile(L, "bc.nl");
  CHECK(st == LUA_ERRSYNTAX);
  CHECK_STR(lua_tostring(L, -1), NOBC);
  lua_pop(L, 1);
  st = luaL_loadfile(L, "bc.nl");
  CHECK(st == LUA_ERRSYNTAX);
  CHECK_STR(lua_tostring(L, -1), NOBC);
  lua_pop(L, 1);
  /* The .nl searcher of the state's own require. */
  CHECK_LUA(L, "select(2, pcall(require, 'bc')):find('" NOBC "', 1, true) ~= nil");
  /* Source still loads. */
  CHECK(donl(L, "pub const v = 1 + 2\n", "=src") == 0);
  CHECK_STR(field(L, "v"), "3");
  lua_pop(L, 1);
  /* The host's own luaL_loadbuffer is not the sandbox's business. */
  CHECK(luaL_loadbuffer(L, luadump, luadumplen, "=luadump") == 0);
  lua_pop(L, 1);
  lua_close(L);
}

static int import_bc(lua_State *L)
{
  nl_import(L, "bc");
  return 1;
}

static void test_bytecode_import(void)
{
  lua_State *L = newstate(1);
  writefile("bc.nl", nldump, nldumplen);
  lua_pushcfunction(L, import_bc);
  CHECK(lua_pcall(L, 0, 1, 0) != 0);
  CHECK_CONTAINS(errtext(L), NOBC);
  lua_close(L);
}

static void test_bytecode_mod(void)
{
  lua_State *L = newstate(1);
  nl_sandboxenv(L);
  lua_pushlstring(L, luadump, luadumplen);
  lua_setfield(L, -2, "LuaDump");
  lua_pushlstring(L, nldump, nldumplen);
  lua_setfield(L, -2, "NlDump");
  lua_pop(L, 1);
  writefile("bcmod.nl", nldump, nldumplen);
  donl(L,
    "global LuaDump\n"
    "global NlDump\n"
    "let f, m1 = load(LuaDump)\n"
    "let g, m2 = loadstring(NlDump, \"=x\")\n"
    "let given = false\n"
    "let h, m3 = load(fn\n"
    "  if given\n"
    "    return nil\n"
    "  end\n"
    "  given = true\n"
    "  return LuaDump\n"
    "end)\n"
    "let f4, m4 = load_nl(NlDump, \"n\")\n"
    "let ok5, m5 = pcall(require, \"bcmod\")\n"
    "let ok6, m6 = pcall(include, \"bcmod.nl\")\n"
    "let srcf = load(\"return 40 + 2\")\n"
    "pub const loaded = f == nil and g == nil and h == nil and f4 == nil\n"
    "pub const e1 = m1\n"
    "pub const e2 = m2\n"
    "pub const e3 = m3\n"
    "pub const e4 = tostring(m4)\n"
    "pub const e5 = tostring(m5)\n"
    "pub const e6 = tostring(m6)\n"
    "pub const src = srcf()\n",
    "=bcmod_user");
  CHECK(fieldbool(L, "loaded") == 1);
  CHECK_STR(field(L, "e1"), NOBC);
  CHECK_STR(field(L, "e2"), NOBC);
  CHECK_STR(field(L, "e3"), NOBC);
  CHECK_CONTAINS(field(L, "e4"), NOBC);
  CHECK_CONTAINS(field(L, "e5"), NOBC);
  CHECK_CONTAINS(field(L, "e6"), NOBC);
  CHECK_STR(field(L, "src"), "42");
  lua_pop(L, 1);
  /* The same from a Lua mod; a Lua file with bytecode through require. */
  writefile("bcmod_lua.lua", luadump, luadumplen);
  CHECK_SBX(L, "select(2, load(LuaDump)) == '" NOBC "'");
  CHECK_SBX(L, "select(2, loadstring(LuaDump, 'x', 'b')) == '" NOBC "'");
  CHECK_SBX(L, "select(2, pcall(require, 'bcmod_lua')):find('" NOBC "', 1, true) ~= nil");
  CHECK_SBX(L, "select(2, pcall(include, 'bcmod_lua.lua')):find('" NOBC "', 1, true) ~= nil");
  CHECK_SBX(L, "load('return ...', 'args')(5) == 5");
  /* string.dump reached through a string's methods: its dump is refused. */
  CHECK_SBX(L, "select(2, load(('').dump(function() return 1 end))) == '" NOBC "'");
  CHECK_SBX(L, "select(2, load_nl(('').dump(function() return 1 end), 'd')) == '" NOBC "'");
  /* A Lua file that is a "#!" line and bytecode (LuaJIT's own message). */
  {
    char *sb = (char *)malloc(luadumplen + 16);
    memcpy(sb, "#!/bin/luajit\n", 14);
    memcpy(sb + 14, luadump, luadumplen);
    writefile("shebang.lua", sb, luadumplen + 14);
    free(sb);
  }
  CHECK_SBX(L, "not pcall(include, 'shebang.lua')");
  CHECK_SBX(L, "not pcall(require, 'shebang')");
  /* A loaded chunk gets the sandbox environment, or the env it is given. */
  CHECK_SBX(L, "load('return _G')() == _G");
  CHECK_SBX(L, "load('return x', 'e', 't', { x = 9 })() == 9");
  lua_close(L);
}

/* -- (b) FFI is off ------------------------------------------------------- */

static void test_ffi(void)
{
  lua_State *L = newstate(1);
  /* The state has the FFI: the host's own Lua code can still require it. */
  CHECK_LUA(L, "package.preload.ffi ~= nil or package.loaded.ffi ~= nil");
  donl(L,
    "let r_ok, r_err = pcall(require, \"ffi\")\n"
    "pub const ok = r_ok\n"
    "pub const err = tostring(r_err)\n"
    "pub const noffi = :ffi == nil\n"
    "pub const nojit = :jit == nil\n",
    "=ffimod");
  CHECK(fieldbool(L, "ok") == 0);
  CHECK_CONTAINS(field(L, "err"), "module 'ffi' is not available in the sandbox");
  CHECK(fieldbool(L, "noffi") == 1);
  CHECK(fieldbool(L, "nojit") == 1);
  lua_pop(L, 1);
  CHECK_SBX(L, "not pcall(require, 'ffi')");
  CHECK_SBX(L, "not pcall(require, 'jit.util')");
  CHECK_SBX(L, "not pcall(require, 'jit.profile')");
  CHECK_SBX(L, "not pcall(require, 'string.buffer')");
  CHECK_SBX(L, "not pcall(require, 'jit')");
  /* Modules the state has loaded are not the sandbox's: */
  CHECK_SBX(L, "not pcall(require, 'os') and not pcall(require, 'io')");
  CHECK_SBX(L, "not pcall(require, 'debug') and not pcall(require, 'package')");
  /* ... and no C module is reachable. */
  CHECK_SBX(L, "select(2, pcall(require, 'nosuchmod')):find('no field package.preload', 1, true) ~= nil");
  CHECK_SBX(L, "select(2, pcall(require, 'nosuchmod')):find('.so', 1, true) == nil");
  lua_close(L);
}

/* -- (c) What a mod can see ----------------------------------------------- */

static void test_env(void)
{
  lua_State *L = newstate(1);
  static const char *const absent[] = {
    "io", "os", "debug", "jit", "ffi", "loadfile", "dofile",
    "collectgarbage", "getfenv", "setfenv", "newproxy", "gcinfo", "module",
    NULL
  };
  int i;
  char buf[128];
  for (i = 0; absent[i]; i++) {
    snprintf(buf, sizeof(buf), "return rawget(_G, '%s') == nil", absent[i]);
    check_count++;
    if (dosandbox(L, buf) != 1 || !lua_toboolean(L, -1)) {
      check_failed_total++; check_test_failed = 1;
      fprintf(stderr, "%s:%d: %s is visible in the sandbox\n", __FILE__, __LINE__, absent[i]);
    }
    lua_settop(L, 0);
  }
  CHECK_SBX(L, "string.dump == nil and type(string.format) == 'function'");
  /* getmetatable hides the metatables shared with the host. */
  CHECK_SBX(L, "getmetatable('') == nil and getmetatable(0) == nil and getmetatable(true) == nil");
  CHECK_SBX(L, "getmetatable(setmetatable({}, { __metatable = 'locked' })) == 'locked'");
  CHECK_SBX(L, "getmetatable(math) == false and getmetatable(table) == false");
  /* The sandbox's own package: preload, loaded, path; nothing else. */
  CHECK_SBX(L, "type(package.preload) == 'table' and type(package.loaded) == 'table' and type(package.path) == 'string'");
  CHECK_SBX(L, "package.cpath == nil and package.loadlib == nil and package.loaders == nil and package.searchers == nil");
  CHECK_SBX(L, "package.seeall == nil and next(package.preload) == nil");
  CHECK_SBX(L, "_G._G == _G");
  CHECK_SBX(L, "('abc'):upper() == 'ABC' and ('%d'):format(5) == '5'");
  CHECK_SBX(L, "table.concat({1, 2}, ',') == '1,2' and math.floor(2.5) == 2");
  CHECK_SBX(L, "bit.band(6, 3) == 2 and pcall(error, 'x') == false");
  CHECK_SBX(L, "type(print) == 'function' and type(coroutine.wrap) == 'function'");
  CHECK_SBX(L, "select('#', 1, 2) == 2 and unpack({1, 2}) == 1");
  /* What NL adds. */
  CHECK_SBX(L, "type(String) == 'table' and type(typeof) == 'function'");
  CHECK_SBX(L, "type(raise) == 'function' and type(include) == 'function'");
  CHECK_SBX(L, "type(load_nl) == 'function' and type(add_path) == 'function'");
  CHECK_SBX(L, "type(keyed) == 'function' and type(event) == 'table'");
  CHECK_SBX(L, "(4).floor ~= nil and String.upcase('a') == 'A'");
  /* Copies of the state's tables, with the same functions. */
  CHECK(dosandbox(L, "return table, math, bit, coroutine, string, math.floor") == 6);
  lua_getglobal(L, "table");
  CHECK(!lua_rawequal(L, -1, -7));
  lua_getglobal(L, "math");
  CHECK(!lua_rawequal(L, -1, -7));
  lua_getglobal(L, "bit");
  CHECK(!lua_rawequal(L, -1, -7));
  lua_getglobal(L, "coroutine");
  CHECK(!lua_rawequal(L, -1, -7));
  lua_getglobal(L, "string");
  CHECK(!lua_rawequal(L, -1, -7));
  lua_getglobal(L, "math");
  lua_getfield(L, -1, "floor");
  CHECK(lua_rawequal(L, -1, -8));
  lua_settop(L, 0);
  /* NL code: the Lua libraries it reaches through prelude names. */
  donl(L,
    "let io_ok, io_err = pcall(fn IO.write(\"x\") end)\n"
    "let os_ok = pcall(fn OS.time() end)\n"
    "let dbg_ok = pcall(fn Debug.traceback() end)\n"
    "let jit_ok = pcall(fn Jit.off() end)\n"
    "pub const reached = io_ok or os_ok or dbg_ok or jit_ok\n"
    "pub const lua = :io == nil and :os == nil and :debug == nil and :jit == nil\n"
    "pub const nopkg = Package.cpath == nil and Package.loadlib == nil\n"
    "pub const fenv = :setfenv == nil and :getfenv == nil and :collectgarbage == nil\n"

    "pub const files = :loadfile == nil and :dofile == nil\n"
    "pub const num = Number.floor(2.5) + 2.7.floor\n"
    "pub const tab = Table.concat([1, 2, 3], \"-\")\n"
    "pub const bits = Bit.bor(1, 2)\n"
    "pub const str = \"hello\".upcase\n"
    "pub const kind = typeof(12)\n"
    "pub const co = Coroutine.wrap(fn return 5 end)()\n",
    "=libs");
  CHECK(fieldbool(L, "reached") == 0);
  CHECK(fieldbool(L, "lua") == 1);
  CHECK(fieldbool(L, "nopkg") == 1);
  CHECK(fieldbool(L, "fenv") == 1);

  CHECK(fieldbool(L, "files") == 1);
  CHECK_STR(field(L, "num"), "4");
  CHECK_STR(field(L, "tab"), "1-2-3");
  CHECK_STR(field(L, "bits"), "3");
  CHECK_STR(field(L, "str"), "HELLO");
  CHECK_STR(field(L, "kind"), "Number");
  CHECK_STR(field(L, "co"), "5");
  lua_pop(L, 1);
  /* The state's own globals are untouched. */
  CHECK_LUA(L, "type(io) == 'table' and type(string.dump) == 'function'");
  CHECK_LUA(L, "type(require('ffi')) == 'table'");
  lua_close(L);
}

/* -- Primitive metatables and the library tables -------------------------- */

static void test_primitives(void)
{
  lua_State *L = newstate(1);
  donl(L,
    ":math.floor = fn(x) 99 end\n"
    ":string.upper = fn(s) \"mod\" end\n"
    "pub const mt_num = :getmetatable(0) == nil\n"
    "pub const mt_str = :getmetatable(\"\") == nil\n"
    "pub const own = Number.floor(2.5)\n"
    "pub const prim = 2.5.floor\n"
    "pub const upper = \"a\".upcase\n"
    "fn Number.sbx_half\n"
    "  self / 2\n"
    "end\n"
    "pub const half = (8).sbx_half\n",
    "=prim_mod");
  CHECK(fieldbool(L, "mt_num") == 1);
  CHECK(fieldbool(L, "mt_str") == 1);
  CHECK_STR(field(L, "own"), "99");     /* The mod's copy of math. */
  CHECK_STR(field(L, "prim"), "2");     /* The state's math, through numbers. */
  CHECK_STR(field(L, "upper"), "A");
  CHECK_STR(field(L, "half"), "4");     /* Defined in the state's math. */
  lua_pop(L, 1);
  /* A Lua mod changing tables it can reach. */
  CHECK_SBX(L, "pcall(function() table.insert = nil; bit.band = nil; coroutine.wrap = nil end)");
  CHECK_SBX(L, "not pcall(function() getmetatable('').__index.upper = nil end)");
  CHECK_SBX(L, "not pcall(function() getmetatable(0).__index.floor = nil end)");
  CHECK_SBX(L, "type(math.sbx_half) == 'function'");  /* Through the copy. */
  /* The host's calls are unchanged. */
  CHECK_LUA(L, "('a'):upper() == 'A' and math.floor(2.5) == 2 and (2.5):floor() == 2");
  CHECK_LUA(L, "string.upper('b') == 'B' and type(table.insert) == 'function'");
  CHECK_LUA(L, "type(bit.band) == 'function' and type(coroutine.wrap) == 'function'");
  lua_close(L);
}

/* -- add_path from a mod -------------------------------------------------- */

static void test_add_path(void)
{
  lua_State *L;
  mkdir("decls", 0777);
  writetext("decls/broken.d.nl", "this is ( not a declaration file\n");
  writetext("decls/fromdecldir.nl", "pub const v = 5\n");
  /* Without the sandbox a directory on the path feeds declaration files
  ** into every compile, so this one breaks them.
  */
  L = newstate(0);
  nl_addpath(L, "decls");
  CHECK(nl_loadbuffer(L, "pub const a = 1\n", 16, "=after") != 0);
  lua_close(L);
  /* In the sandbox, add_path widens the import search and nothing else. */
  L = newstate(1);
  donl(L, "add_path(\"decls\")\n", "=adder");
  lua_pop(L, 1);
  donl(L, "import from 'fromdecldir' as D\npub const v = D.v\n", "=importer");
  CHECK_STR(field(L, "v"), "5");
  lua_pop(L, 1);
  CHECK_LUA(L, "load_nl('pub const a = 2', 'host')().a == 2");
  CHECK(nl_loadbuffer(L, "pub const a = 1\n", 16, "=host") == 0);
  lua_pop(L, 1);
  lua_close(L);
}

/* -- The environment made before luaL_openlibs ---------------------------- */

static void test_early_env(void)
{
  lua_State *L = luaL_newstate();
  nl_sandboxenv(L);
  lua_pop(L, 1);
  lua_pushcfunction(L, openlibs);
  CHECK(lua_pcall(L, 0, 0, 0) == 0);
  CHECK(nl_setoption(L, NL_OPT_SANDBOX, 1) == 1);
  donl(L,
    "let n = 0\n"
    "for k, v in pairs({a: 1, b: 2})\n"
    "  n += v\n"
    "end\n"
    "pub const n = n\n"
    "pub const printer = :type(:print) == \"function\"\n"
    "pub const libs = :type(:math) == \"table\" and :type(:string) == \"table\"\n",
    "=early");
  CHECK_STR(field(L, "n"), "3");
  CHECK(fieldbool(L, "printer") == 1);
  CHECK(fieldbool(L, "libs") == 1);
  lua_pop(L, 1);
  /* A name a mod deletes comes back on the next load; its own stay. */
  CHECK_SBX(L, "pcall(function() print = nil; pairs = 'mine' end)");
  CHECK_SBX(L, "type(print) == 'function' and pairs == 'mine'");
  lua_close(L);
}

/* -- The sandbox's package ------------------------------------------------ */

static int preload_sbxmod(lua_State *L)
{
  lua_newtable(L);
  lua_pushinteger(L, 1);
  lua_setfield(L, -2, "v");
  return 1;
}

static void test_package(void)
{
  lua_State *L = newstate(1);
  /* A bundler's privileged module in the state's own preload. */
  CHECK(dolua(L, "package.preload.secret = function() return io end") == 0);
  /* The host's module for mods, in the sandbox's preload. */
  nl_sandboxenv(L);
  lua_getfield(L, -1, "package");
  lua_getfield(L, -1, "preload");
  lua_pushcfunction(L, preload_sbxmod);
  lua_setfield(L, -2, "sbxmod");
  lua_pop(L, 3);
  donl(L,
    "let m = require(\"sbxmod\")\n"
    "let ok, err = pcall(require, \"secret\")\n"
    "pub const v = m.v\n"
    "pub const secret = ok\n"
    "pub const cached = :package.loaded.sbxmod == m\n",
    "=pkg_mod");
  CHECK_STR(field(L, "v"), "1");
  CHECK(fieldbool(L, "secret") == 0);
  CHECK(fieldbool(L, "cached") == 1);
  lua_pop(L, 1);
  CHECK_LUA(L, "require('secret') == io");  /* The host's own require. */
  lua_close(L);
}

/* -- Files through the host's loader -------------------------------------- */

static const char *ld_file(const char *name)
{
  if (strcmp(name, "cfg.lua") == 0) return "return 'from the loader'";
  if (strcmp(name, "./lmod.lua") == 0) return "return { n = 3 }";
  if (strcmp(name, "mods/m.nl") == 0) return "pub const m = 1\n";
  return NULL;
}
static int ld_resolve(void *d, const char *from, const char *path, char *out, size_t size)
{
  char name[256];
  (void)d; (void)from;
  snprintf(name, sizeof(name), "mods/%s.nl", path);
  if (!ld_file(name)) return 0;
  snprintf(out, size, "%s", name);
  return 1;
}
static const char *ld_read(void *d, const char *name, size_t *size)
{
  const char *s = ld_file(name);
  (void)d;
  if (!s) return NULL;
  *size = strlen(s);
  return s;
}

static void test_loader_files(void)
{
  lua_State *L = newstate(1);
  nl_Loader ld;
  ld.resolve = ld_resolve;
  ld.read = ld_read;
  ld.release = NULL;
  ld.data = NULL;
  nl_setloader(L, &ld);
  writetext("disk.lua", "return 'from the disk'");
  writetext("lmod_disk.lua", "return 'from the disk'");
  donl(L,
    "pub const cfg = include(\"cfg.lua\")\n"
    "pub const lmod = require(\"lmod\").n\n"
    "let d_ok, d_err = pcall(include, \"disk.lua\")\n"
    "let r_ok, r_err = pcall(require, \"lmod_disk\")\n"
    "pub const disk = d_ok or r_ok\n"
    "pub const d_err = tostring(d_err)\n",
    "=loader_mod");
  CHECK_STR(field(L, "cfg"), "from the loader");
  CHECK_STR(field(L, "lmod"), "3");
  CHECK(fieldbool(L, "disk") == 0);
  CHECK_CONTAINS(field(L, "d_err"), "cannot open disk.lua");
  lua_pop(L, 1);
  lua_close(L);
}

/* -- (d) Mods share globals ----------------------------------------------- */

static void test_shared_globals(void)
{
  lua_State *L = newstate(1);
  donl(L, "global SharedScore = 41\n:_G.shared_fn = fn(x) x + 1 end\n", "=mod_a");
  lua_pop(L, 1);
  donl(L,
    "global SharedScore\n"
    "SharedScore += 1\n"
    "pub const score = SharedScore\n"
    "pub const called = :shared_fn(1)\n",
    "=mod_b");
  CHECK_STR(field(L, "score"), "42");
  CHECK_STR(field(L, "called"), "2");
  lua_pop(L, 1);
  /* In the sandbox environment, not in the state's globals. */
  lua_getglobal(L, "SharedScore");
  CHECK(lua_isnil(L, -1));
  lua_pop(L, 1);
  nl_sandboxenv(L);
  lua_getfield(L, -1, "SharedScore");
  CHECK(lua_tonumber(L, -1) == 42);
  lua_pop(L, 2);
  /* A Lua mod sees them too, and import/require from a mod are sandboxed. */
  CHECK_SBX(L, "SharedScore == 42");
  writetext("sbx_lib.nl", "global FromLib = \"lib\"\npub fn twice(x)\n  x * 2\nend\n");
  writetext("sbx_lualib.lua", "LuaLibLoaded = true\nreturn { three = function() return 3 end }\n");
  donl(L,
    "import from 'sbx_lib' as Lib\n"
    "let lualib = require(\"sbx_lualib\")\n"
    "pub const twice = Lib.twice(4)\n"
    "pub const three = lualib.three()\n",
    "@sbx_user.nl");
  CHECK_STR(field(L, "twice"), "8");
  CHECK_STR(field(L, "three"), "3");
  lua_pop(L, 1);
  CHECK_SBX(L, "FromLib == 'lib' and LuaLibLoaded == true");
  CHECK_SBX(L, "require('sbx_lualib') == require('sbx_lualib')");
  CHECK_LUA(L, "FromLib == nil and LuaLibLoaded == nil");
  lua_close(L);
}

/* -- (e) The host adds to the environment --------------------------------- */

static int host_greet(lua_State *L)
{
  lua_pushfstring(L, "hello, %s", luaL_checkstring(L, 1));
  return 1;
}

static void test_host_function(void)
{
  lua_State *L = newstate(1);
  int top = lua_gettop(L);
  nl_sandboxenv(L);
  CHECK(lua_istable(L, -1));
  CHECK(lua_gettop(L) == top + 1);
  lua_pushcfunction(L, host_greet);
  lua_setfield(L, -2, "greet");
  lua_pop(L, 1);
  donl(L, "pub const said = :greet(\"mod\")\n", "=greeter");
  CHECK_STR(field(L, "said"), "hello, mod");
  lua_pop(L, 1);
  /* One table per state, also before the option is set. */
  {
    lua_State *L2 = newstate(0);
    nl_sandboxenv(L2);
    nl_sandboxenv(L2);
    CHECK(lua_rawequal(L2, -1, -2));
    lua_getfield(L2, -1, "_G");
    CHECK(lua_rawequal(L2, -1, -2));
    lua_close(L2);
  }
  lua_close(L);
}

/* -- (f) With the option off nothing changes ------------------------------ */

static void test_off(void)
{
  lua_State *L = newstate(0);
  donl(L,
    "global OffGlobal = 1\n"
    "pub const full = :io != nil and :debug != nil\n",
    "=offmod");
  CHECK(fieldbool(L, "full") == 1);
  lua_pop(L, 1);
  CHECK_LUA(L, "OffGlobal == 1");
  /* An NL dump still fails in nl_loadbuffer, as the compiler's error. */
  CHECK(nl_loadbuffer(L, nldump, nldumplen, "=nldump") == LUA_ERRSYNTAX);
  CHECK(strcmp(lua_tostring(L, -1), NOBC) != 0);
  lua_pop(L, 1);
  CHECK_LUA(L, "load(string.dump(function() return 3 end))() == 3");
  CHECK_LUA(L, "type(load_nl('pub const a = 1', 'x')().a) == 'number'");
  lua_close(L);
}

int main(int argc, char **argv)
{
  if (argc > 1 && chdir(argv[1]) != 0) { perror(argv[1]); return 2; }
  NL_VERSION_SYM();
  make_dumps();
  RUN_TEST(test_option);
  RUN_TEST(test_localize);
  RUN_TEST(test_bytecode_host);
  RUN_TEST(test_bytecode_import);
  RUN_TEST(test_bytecode_mod);
  RUN_TEST(test_ffi);
  RUN_TEST(test_env);
  RUN_TEST(test_primitives);
  RUN_TEST(test_add_path);
  RUN_TEST(test_early_env);
  RUN_TEST(test_package);
  RUN_TEST(test_loader_files);
  RUN_TEST(test_shared_globals);
  RUN_TEST(test_host_function);
  RUN_TEST(test_off);
  free(luadump);
  free(nldump);
  return check_summary("test_sandbox");
}
