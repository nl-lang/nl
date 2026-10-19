/*
** A C host with workers: a host type that crosses
** between states with its transfer pair, a worker-init function, nl_spawn
** and nl_drain. src/bin/workers.rs is its Rust twin; run.sh runs both with
** 0, 1 and 4 pool workers on scripts/ and compares what they print.
**
**   host N        (in the directory that holds scripts/)
*/
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "lua.h"
#include "lauxlib.h"
#include "lualib.h"
#include "nl.h"

typedef struct Vec2 { double x, y; } Vec2;

static Vec2 *pushvec2(lua_State *L, double x, double y) {
  Vec2 *v = (Vec2 *)lua_newuserdata(L, sizeof *v);
  v->x = x;
  v->y = y;
  luaL_getmetatable(L, "Vec2");
  lua_setmetatable(L, -2);
  return v;
}

static int vec2_scale(lua_State *L) {         /* vec2_scale(v, k) */
  Vec2 *v = (Vec2 *)luaL_checkudata(L, 1, "Vec2");
  lua_Number k = luaL_checknumber(L, 2);
  pushvec2(L, v->x * k, v->y * k);
  return 1;
}

static int vec2_str(lua_State *L) {           /* "x,y" */
  Vec2 *v = (Vec2 *)luaL_checkudata(L, 1, "Vec2");
  lua_pushfstring(L, "%f,%f", v->x, v->y);
  return 1;
}

static int host_isworker(lua_State *L) {
  lua_pushboolean(L, nl_isworker(L));
  return 1;
}

static int vec2_encode(lua_State *L) {
  Vec2 *v = (Vec2 *)luaL_checkudata(L, 1, "Vec2");
  lua_pushlstring(L, (const char *)v, sizeof *v);
  return 1;
}

static int vec2_decode(lua_State *L) {
  size_t len;
  const char *s = luaL_checklstring(L, 1, &len);
  if (len != sizeof(Vec2)) return luaL_error(L, "bad Vec2");
  pushvec2(L, 0, 0);
  memcpy(lua_touserdata(L, -1), s, sizeof(Vec2));
  return 1;
}

static void register_host(lua_State *L) {     /* the main state, and each worker */
  luaL_newmetatable(L, "Vec2");
  nl_settype(L, -1, "Vec2", 0);
  lua_pop(L, 1);
  lua_pushcfunction(L, vec2_encode);
  lua_pushcfunction(L, vec2_decode);
  nl_settransfer(L, "Vec2");
  lua_register(L, "vec2_scale", vec2_scale);
  lua_register(L, "vec2_str", vec2_str);
  lua_register(L, "host_isworker", host_isworker);
}

static int worker_init(lua_State *W) {        /* in each new worker, on its thread */
  register_host(W);
  return 0;
}

static int on_unhandled(lua_State *L) {
  fprintf(stderr, "[host] unhandled: %s\n", nlL_tolstring(L, 1, NULL));
  return 0;
}

static int setup(lua_State *L) {
  luaL_openlibs(L);
  register_host(L);
  lua_pushcfunction(L, on_unhandled);
  nl_setunhandled(L);
  nl_addpath(L, "scripts");
  return 0;
}

static int spawn_triple(lua_State *L) {      /* nl_spawn raises: under nl_pcall */
  lua_getfield(L, 1, "triple");
  lua_pushnumber(L, 12);
  nl_spawn(L, 1);
  return 1;
}

static void report(lua_State *L) {
  fprintf(stderr, "%s\n", nlL_tolstring(L, -1, NULL));
}

int main(int argc, char **argv) {
  lua_State *L;
  int workers = argc > 1 ? atoi(argv[1]) : 1;
  int exports, st;
  NL_VERSION_SYM();
  L = luaL_newstate();
  if (lua_cpcall(L, setup, NULL) != 0) { report(L); return 1; }
  nl_setworkers(L, workers);
  nl_setworkerinit(L, worker_init, NULL);
  printf("[host] the main state is a worker: %s\n", nl_isworker(L) ? "yes" : "no");

  if (nl_loadfile(L, "scripts/main.nl") != 0 || nl_pcall(L, 0, 1) != 0) { report(L); return 1; }
  exports = lua_gettop(L);

  lua_getfield(L, exports, "run");             /* run(Vec2 { 1, 2 }): a Task */
  pushvec2(L, 1, 2);
  if (nl_pcall(L, 1, 1) != 0) { report(L); return 1; }
  while ((st = nl_taskstate(L, -1)) == NL_TASK_PENDING) {
    nl_drain(L);
  }
  lua_settop(L, exports + 1);
  if (st == NL_TASK_DONE) {
    int i, n;
    nl_taskstate(L, -1);
    n = (int)lua_objlen(L, -1);
    for (i = 1; i <= n; i++) {
      lua_rawgeti(L, -1, i);
      printf("[script] %s\n", lua_tostring(L, -1));
      lua_pop(L, 1);
    }
  } else {
    nl_taskstate(L, -1);
    report(L);
  }
  lua_settop(L, exports);

  lua_pushcfunction(L, spawn_triple);          /* nl_spawn from the host */
  lua_pushvalue(L, exports);
  if (nl_pcall(L, 1, 1) != 0) { report(L); return 1; }
  nl_drain(L);
  if (nl_taskstate(L, -1) == NL_TASK_DONE)
    printf("[host] spawned triple(12): %s\n", lua_tostring(L, -1));
  lua_settop(L, exports);

  lua_close(L);
  return 0;
}
