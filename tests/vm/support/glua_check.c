/* glua_check [-g] [-q] (-e CODE | FILE)...: loads each chunk as Lua, with
** NL_OPT_GLUA on after -g (off after -G), and prints "ok NAME" or
** "ERR NAME: message"; -q prints the errors only. The chunks are not run.
** Exit status: the number of failures, at most 100. Built and run by
** tests/vm/glua_corpus.sh.
*/
#include <stdio.h>
#include <string.h>
#include "lua.h"
#include "lauxlib.h"
#include "lualib.h"
#include "nl.h"

int main(int argc, char **argv)
{
  int i, fails = 0, quiet = 0;
  lua_State *L = luaL_newstate();
  luaL_openlibs(L);
  for (i = 1; i < argc; i++) {
    int st;
    const char *name = argv[i];
    if (!strcmp(name, "-g")) { nl_setoption(L, NL_OPT_GLUA, 1); continue; }
    if (!strcmp(name, "-G")) { nl_setoption(L, NL_OPT_GLUA, 0); continue; }
    if (!strcmp(name, "-q")) { quiet = 1; continue; }
    if (!strcmp(name, "-e") && i+1 < argc) {
      name = argv[++i];
      st = luaL_loadstring(L, name);
    } else {
      st = luaL_loadfile(L, name);
    }
    if (st != 0) {
      fails++;
      printf("ERR %s: %s\n", name, lua_tostring(L, -1));
    } else if (!quiet) {
      printf("ok %s\n", name);
    }
    lua_settop(L, 0);
  }
  lua_close(L);
  return fails > 100 ? 100 : fails;
}
