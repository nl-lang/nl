/*
** The end-to-end test: a host program loads .nl and
** .lua files side by side, and they require/import each other. Built against
** the real libnlc (crates/nl_capi) and the real runtime (the .lua files of runtime/) by
** tests/vm/e2e/run.sh. Run with tests/vm/e2e/project as the working
** directory.
*/

#include <stdio.h>
#include <string.h>

#include "lua.h"
#include "lauxlib.h"
#include "lualib.h"
#include "nl.h"

static int failures;

static void reporter(void *data, const nl_Diagnostic *d)
{
  (void)data;
  fprintf(stderr, "  diagnostic: %s:%d:%d: [%s] %s\n", d->file, d->line,
	  d->column, d->code, d->message);
}

static void expect(int ok, const char *what)
{
  printf("%s - %s\n", ok ? "ok" : "not ok", what);
  if (!ok) failures++;
}

static int openlibs(lua_State *L)
{
  luaL_openlibs(L);
  return 0;
}

static const char *errstr(lua_State *L)
{
  return lua_isstring(L, -1) ? lua_tostring(L, -1) : nlL_tolstring(L, -1, NULL);
}

int main(void)
{
  lua_State *L;
  NL_VERSION_SYM();
  L = luaL_newstate();
  if (!L) return 2;
  nl_setreporter(L, reporter, NULL);
  lua_pushcfunction(L, openlibs);
  if (lua_pcall(L, 0, 0, 0) != 0) {
    printf("not ok - luaL_openlibs (opening the runtime): %s\n", lua_tostring(L, -1));
    return 1;
  }
  expect(1, "luaL_openlibs opens NL");

  /* 1. An NL main file, loaded by the host. */
  if (nlL_dofile(L, "main.nl")) {
    printf("not ok - nlL_dofile(main.nl): %s\n", errstr(L));
    lua_close(L);
    return 1;
  }
  expect(lua_istable(L, -1), "main.nl returns its export table");
  lua_getfield(L, -1, "summary");
  if (nl_pcall(L, 0, 1) != 0) {
    printf("not ok - summary(): %s\n", errstr(L));
    failures++;
  } else {
    const char *s = lua_tostring(L, -1);
    printf("# summary() = %s\n", s ? s : "(not a string)");
    expect(s && strcmp(s, "hello host, 5, hello lua, runs 1") == 0,
	   "NL imports NL, requires Lua, which requires NL; one run per file");
  }
  lua_pop(L, 1);

  /* 2. import and require give the same export table. */
  nl_import(L, "nl/greeter");
  lua_getglobal(L, "require");
  lua_pushliteral(L, "nl.greeter");
  if (lua_pcall(L, 1, 1, 0) != 0) {
    printf("not ok - require('nl.greeter'): %s\n", errstr(L));
    failures++;
  } else {
    expect(lua_rawequal(L, -1, -2), "nl_import and require share one export table");
  }
  lua_settop(L, 1);

  /* 3. An NL exception reaches the host as an Exception. */
  lua_getfield(L, 1, "fail");
  expect(nl_pcall(L, 0, 0) != 0, "an NL raise fails nl_pcall");
  nl_gettype(L, "Exception");
  expect(nl_isa(L, -2, -1), "the value is an Exception");
  lua_getfield(L, -2, "message");
  expect(lua_isstring(L, -1) && strcmp(lua_tostring(L, -1), "raised in main.nl") == 0,
	 "with its message");
  lua_settop(L, 0);

  /* 4. A Lua main file that requires and includes NL files. */
  if (luaL_dofile(L, "main.lua")) {
    printf("not ok - luaL_dofile(main.lua): %s\n", errstr(L));
    failures++;
  } else {
    expect(1, "main.lua runs (its output is compared by run.sh)");
  }
  lua_settop(L, 0);

  lua_close(L);
  printf("# e2e host: %d failed\n", failures);
  return failures ? 1 : 0;
}
