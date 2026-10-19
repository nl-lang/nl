/*
** The host of the benchmarks (bench/run.sh): runs one program on the fork,
** NL or Lua, in the same process setup, so that the two sides of a pair
** differ only in the file.
**
**   nlbench [--unchecked] [--list] FILE.nl|FILE.lua
**
** - the state is opened as tests/vm/conform/nlrun.c opens it
**   (luaL_openlibs, which opens the NL runtime);
** - --unchecked sets NL_OPT_CHECKS to 0 before the file loads, which is
**   the unchecked build (`nlc build --unchecked`);
** - the file is loaded with luaL_loadfile, which picks the language by the
**   extension, and run with nl_pcall; an error is printed on stderr, exit
**   status 1;
** - the environment variable NLB_LOCALIZE, when set, is NL_OPT_LOCALIZE
**   (0, 1 or 2), so that a run can compare NL code with and
**   without localized Lua paths (the default is the option's, 1);
** - --list prints the bytecode of the loaded chunk instead of running it
**   (jit.bc, as `luajit -bl` does; needs LUA_PATH to reach the build's
**   jit/ directory);
** - the benchmarks start no task, so there is nothing to pump afterwards.
*/

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "lua.h"
#include "lauxlib.h"
#include "lualib.h"
#include "nl.h"

static int openlibs(lua_State *L)
{
  luaL_openlibs(L);
  return 0;
}

static void print_error(lua_State *L, const char *what)
{
  const char *msg = lua_type(L, -1) == LUA_TSTRING ? lua_tostring(L, -1) : nlL_tolstring(L, -1, NULL);
  fprintf(stderr, "%s: %s\n", what, msg ? msg : "(no message)");
  lua_settop(L, 0);
}

int main(int argc, char **argv)
{
  lua_State *L;
  int unchecked = 0, list = 0;
  while (argc > 2 && argv[1][0] == '-') {
    if (strcmp(argv[1], "--unchecked") == 0) unchecked = 1;
    else if (strcmp(argv[1], "--list") == 0) list = 1;
    else break;
    argv++;
    argc--;
  }
  if (argc != 2) {
    fprintf(stderr, "usage: nlbench [--unchecked] [--list] FILE.nl|FILE.lua\n");
    return 2;
  }
  NL_VERSION_SYM();
  L = luaL_newstate();
  lua_pushcfunction(L, openlibs);
  if (lua_pcall(L, 0, 0, 0) != 0) {
    print_error(L, "luaL_openlibs");
    return 1;
  }
  if (unchecked && !nl_setoption(L, NL_OPT_CHECKS, 0)) {
    fprintf(stderr, "nlbench: cannot set NL_OPT_CHECKS\n");
    return 1;
  }
  if (getenv("NLB_LOCALIZE") &&
      !nl_setoption(L, NL_OPT_LOCALIZE, atoi(getenv("NLB_LOCALIZE")))) {
    fprintf(stderr, "nlbench: cannot set NL_OPT_LOCALIZE\n");
    return 1;
  }
  if (luaL_loadfile(L, argv[1]) != 0) {
    print_error(L, argv[1]);
    return 1;
  }
  if (list) {
    lua_getglobal(L, "require");
    lua_pushstring(L, "jit.bc");
    if (lua_pcall(L, 1, 1, 0) != 0) {
      print_error(L, "require 'jit.bc'");
      return 1;
    }
    lua_getfield(L, -1, "dump");
    lua_pushvalue(L, 1);
    lua_pushnil(L);
    lua_pushboolean(L, 1);
    if (lua_pcall(L, 3, 0, 0) != 0) {
      print_error(L, "jit.bc.dump");
      return 1;
    }
    lua_close(L);
    return 0;
  }
  if (nl_pcall(L, 0, 0) != 0) {
    if (lua_istable(L, -1)) {
      lua_getfield(L, -1, "trace");
      if (lua_isstring(L, -1)) fprintf(stderr, "%s\n", lua_tostring(L, -1));
      lua_pop(L, 1);
    }
    print_error(L, "uncaught");
    fflush(stdout);
    return 1;
  }
  fflush(stdout);
  lua_close(L);
  return 0;
}
