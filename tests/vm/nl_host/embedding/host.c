#include <stdio.h>
#include <string.h>

#include "lua.h"
#include "lauxlib.h"
#include "lualib.h"
#include "nl.h"

/* -- Errors -------------------------------------------------------------- */

static int tostring_f(lua_State *L) {
  nlL_tolstring(L, 1, NULL);
  return 1;
}

static void report(lua_State *L) {             /* pops the error value */
  int err = lua_gettop(L);
  const char *s;
  lua_pushcfunction(L, tostring_f);
  lua_pushvalue(L, err);
  lua_pcall(L, 1, 1, 0);
  s = lua_tostring(L, -1);
  fprintf(stderr, "%s\n", s ? s : "(error value has no text)");
  if (lua_istable(L, err)) {
    lua_getfield(L, err, "trace");
    if (lua_isstring(L, -1)) fprintf(stderr, "%s\n", lua_tostring(L, -1));
  }
  lua_settop(L, err - 1);
}

static void reporter(void *data, const nl_Diagnostic *d) {
  static const char *const sev[] = { "error", "warning", "lint" };
  (void)data;
  fprintf(stderr, "%s:%d:%d: %s: %s [%s]\n", d->file, d->line, d->column,
          sev[d->severity], d->message, d->code);
}

/* -- The host type Unit -------------------------------------------------- */

typedef struct Unit {
  char name[32];
  double hp;
} Unit;

static int unit_damage(lua_State *L) {         /* NL: u.damage 5 */
  Unit *u = (Unit *)luaL_checkudata(L, 1, "Unit");
  lua_Number amount = luaL_checknumber(L, 2);
  if (amount < 0)
    return nlL_raise(L, "TypeError", "amount must not be negative, got %f", amount);
  if (u->hp <= 0) return 0;
  u->hp -= amount;
  if (u->hp <= 0) {
    u->hp = 0;
    lua_pushvalue(L, 1);
    nl_runevent(L, "UnitDied", 1, 0);          /* on UnitDied ... */
  }
  return 0;
}

static int unit_hp(lua_State *L) {             /* NL: u.hp, auto-called */
  lua_pushnumber(L, ((Unit *)luaL_checkudata(L, 1, "Unit"))->hp);
  return 1;
}

static int unit_tostring(lua_State *L) {       /* "#{u}" */
  Unit *u = (Unit *)luaL_checkudata(L, 1, "Unit");
  lua_pushfstring(L, "%s, %f hp", u->name, u->hp);
  return 1;
}

static const luaL_Reg unit_methods[] = {
  { "damage", unit_damage },
  { "hp",     unit_hp },
  { NULL, NULL }
};

static int engine_new_unit(lua_State *L) {     /* NL: Engine.new_unit("orc 1", 10) */
  const char *name = luaL_checkstring(L, 1);
  lua_Number hp = luaL_checknumber(L, 2);
  Unit *u = (Unit *)lua_newuserdata(L, sizeof(Unit));
  snprintf(u->name, sizeof(u->name), "%s", name);
  u->hp = hp;
  luaL_getmetatable(L, "Unit");
  lua_setmetatable(L, -2);
  return 1;
}

static int engine_log(lua_State *L) {
  printf("[script] %s\n", luaL_checkstring(L, 1));
  return 0;
}

static const luaL_Reg engine_funcs[] = {
  { "new_unit", engine_new_unit },
  { "log",      engine_log },
  { NULL, NULL }
};

/* -- Events and failures ------------------------------------------------- */

static int on_unit_died(lua_State *L) {
  printf("[host] %s died\n", nlL_tolstring(L, 1, NULL));
  return 0;                                    /* let the script's handler run too */
}

static int on_unhandled(lua_State *L) {
  fprintf(stderr, "[host] unhandled: %s\n", nlL_tolstring(L, 1, NULL));
  return 0;
}

/* -- Task.sleep in game time --------------------------------------------- */

#define MAXTIMERS 64

typedef struct Timer {
  double due;
  nl_Callback wake;
  void *arg;
} Timer;

static Timer timers[MAXTIMERS];
static int ntimers;
static double game_time;

static void start_timer(void *data, lua_State *L, double seconds,
                        nl_Callback wake, void *arg) {
  (void)data;
  if (ntimers == MAXTIMERS) {
    luaL_error(L, "too many sleeping tasks");  /* runs inside Task.sleep: the task fails */
    return;
  }
  timers[ntimers].due = game_time + seconds;
  timers[ntimers].wake = wake;
  timers[ntimers].arg = arg;
  ntimers++;
}

static void run_timers(lua_State *L) {
  Timer due[MAXTIMERS];
  int i = 0, n = 0;
  while (i < ntimers) {                        /* take the due ones out first */
    if (timers[i].due <= game_time) {
      due[n++] = timers[i];
      timers[i] = timers[--ntimers];
    } else {
      i++;
    }
  }
  for (i = 0; i < n; i++)                      /* a sleep started here waits a frame */
    due[i].wake(L, due[i].arg);
}

/* -- Setup, under lua_cpcall --------------------------------------------- */

static int setup(lua_State *L) {
  luaL_openlibs(L);                            /* opens NL as well */
  nl_addpath(L, "scripts");                    /* imports, and scripts/game.d.nl */

  luaL_newmetatable(L, "Unit");
  nl_settype(L, -1, "Unit", 0);                /* typeof, `is Unit`, `u: Unit` */
  lua_pushcfunction(L, unit_tostring);
  lua_setfield(L, -2, "__tostring");
  lua_newtable(L);
  nlL_setmethods(L, unit_methods);             /* marked: u.damage 5, u.hp */
  lua_setfield(L, -2, "__index");
  lua_pop(L, 1);

  lua_newtable(L);                             /* plain functions: not marked */
  luaL_register(L, NULL, engine_funcs);
  lua_setglobal(L, "engine");

  lua_pushcfunction(L, on_unit_died);
  nl_onevent(L, "UnitDied", "host:log");
  lua_pushcfunction(L, on_unhandled);
  nl_setunhandled(L);
  nl_settimer(L, start_timer, NULL);
  return 0;
}

/* -- Main ---------------------------------------------------------------- */

int main(void) {
  lua_State *L;
  int frame;

  NL_VERSION_SYM();
  L = luaL_newstate();
  if (!L) return 1;
  nl_setreporter(L, reporter, NULL);
  if (lua_cpcall(L, setup, NULL) != 0) {
    report(L);
    lua_close(L);
    return 1;
  }

  if (nl_loadfile(L, "scripts/main.nl") != 0) {
    fprintf(stderr, "%s\n", lua_tostring(L, -1));
    lua_close(L);
    return 1;
  }
  if (nl_pcall(L, 0, 1) != 0) {                /* runs main.nl: its export table */
    report(L);
    lua_close(L);
    return 1;
  }
  lua_getfield(L, -1, "start");
  lua_pushnumber(L, 3);
  if (nl_pcall(L, 1, 0) != 0) report(L);       /* start(3) */
  lua_pop(L, 1);

  for (frame = 0; frame < 50; frame++) {       /* the game: 5 s at 10 frames a second */
    game_time += 0.1;
    lua_pushnumber(L, 0.1);
    if (nl_prunevent(L, "Game.Tick", 1, 0) != 0) report(L);
    run_timers(L);
    nl_pump(L);
  }

  for (;;) {                                   /* shutdown: finish what is pending */
    game_time += 0.1;
    run_timers(L);
    if (nl_pump(L) == 0 && ntimers == 0) break;
  }

  lua_close(L);
  return 0;
}
