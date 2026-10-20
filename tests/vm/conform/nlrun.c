/*
** Runs one NL program on the fork as a host would, with the rules of
** tests/run/README.md and nlc run (crates/nlc/src/run.rs):
**   nlrun [--sandbox] [--decl PATH]... FILE.nl
** - with --sandbox, NL_OPT_SANDBOX is set before the file loads, so the
**   program runs as an untrusted mod would;
** - --decl names a declaration file or a directory of them, as for
**   nlc run --decl: a file goes to nl_adddecl as given, a directory's
**   .d.nl files in name order (the suite passes --decl decl);
** - the file is loaded with luaL_loadfile (NL by its extension) and run
**   with nl_pcall; an uncaught error is printed on stderr, exit status 1;
** - with NL_CONFORM_WORKERS=n in the environment, nl_setworkers(L, n)
**   before the file loads (0: jobs and dedicated workers run on this thread
**   while it pumps);
** - then it pumps (nl_pump, which drives the runtime's queue), fires the
**   timers of Task.sleep (real time, in the order of their deadlines) and
**   drains what workers still owe (nl_drain) until nothing is left; a task
**   still pending then is an error (exit status 1).
**
** The number of pending tasks is rt.pending() of runtime/nl.lua, which the
** C API does not offer: this test program finds rt in the registry (the
** table that has `pending` and `runmain`). Hosts have no need for it.
*/

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <dirent.h>
#include <sys/stat.h>

#include "lua.h"
#include "lauxlib.h"
#include "lualib.h"
#include "nl.h"

typedef struct Sleep {
  double at;
  long seq;
  nl_Callback wake;
  void *arg;
} Sleep;

static Sleep sleeps[1024];
static int nsleeps;
static long seq;

static double now(void)
{
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return ts.tv_sec + ts.tv_nsec / 1e9;
}

static void timer(void *data, lua_State *L, double seconds, nl_Callback wake, void *arg)
{
  (void)data; (void)L;
  if (nsleeps == (int)(sizeof(sleeps) / sizeof(sleeps[0]))) {
    fprintf(stderr, "nlrun: too many sleeps\n");
    exit(1);
  }
  if (!(seconds > 0)) seconds = 0;
  if (seconds > 1e6) seconds = 1e6;
  sleeps[nsleeps].at = now() + seconds;
  sleeps[nsleeps].seq = ++seq;
  sleeps[nsleeps].wake = wake;
  sleeps[nsleeps].arg = arg;
  nsleeps++;
}

/* Waits for the earliest sleep and wakes it; 0 when there is none. */
static int fire_timer(lua_State *L)
{
  int i, best = -1;
  double t;
  for (i = 0; i < nsleeps; i++)
    if (best < 0 || sleeps[i].at < sleeps[best].at ||
	(sleeps[i].at == sleeps[best].at && sleeps[i].seq < sleeps[best].seq))
      best = i;
  if (best < 0) return 0;
  {
    Sleep s = sleeps[best];
    sleeps[best] = sleeps[--nsleeps];
    t = s.at - now();
    if (t > 0) {
      struct timespec ts;
      ts.tv_sec = (time_t)t;
      ts.tv_nsec = (long)((t - (double)ts.tv_sec) * 1e9);
      nanosleep(&ts, NULL);
    }
    s.wake(L, s.arg);
  }
  return 1;
}

static int pending_tasks(lua_State *L)
{
  static const char *code =
    "for _, v in pairs(debug.getregistry()) do\n"
    "  if type(v) == 'table' and type(rawget(v, 'pending')) == 'function'\n"
    "     and type(rawget(v, 'runmain')) == 'function' then return v.pending() end\n"
    "end\n"
    "return -1";
  int n = -1;
  if (luaL_loadstring(L, code) == 0 && lua_pcall(L, 0, 1, 0) == 0)
    n = (int)lua_tointeger(L, -1);
  lua_settop(L, 0);
  return n;
}

static int name_cmp(const void *a, const void *b)
{
  return strcmp(*(char *const *)a, *(char *const *)b);
}

/* --decl PATH: a .d.nl file, or every .d.nl of a directory in name order. */
static int add_decl(lua_State *L, const char *path)
{
  struct stat st;
  DIR *d;
  struct dirent *e;
  char *names[256];
  int n = 0, i;
  if (stat(path, &st) != 0) {
    fprintf(stderr, "nlrun: --decl %s: no such file or directory\n", path);
    return 0;
  }
  if (!S_ISDIR(st.st_mode)) {
    nl_adddecl(L, path);
    return 1;
  }
  if (!(d = opendir(path))) {
    fprintf(stderr, "nlrun: --decl %s: cannot read the directory\n", path);
    return 0;
  }
  while ((e = readdir(d)) != NULL && n < (int)(sizeof(names) / sizeof(names[0]))) {
    size_t len = strlen(e->d_name);
    if (len > 5 && strcmp(e->d_name + len - 5, ".d.nl") == 0) {
      names[n] = malloc(strlen(path) + len + 2);
      sprintf(names[n++], "%s/%s", path, e->d_name);
    }
  }
  closedir(d);
  qsort(names, n, sizeof(names[0]), name_cmp);
  for (i = 0; i < n; i++) {
    nl_adddecl(L, names[i]);
    free(names[i]);
  }
  return 1;
}

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
  int n, i, sandbox = 0;
  const char *decls[16];
  int ndecls = 0;
  while (argc > 2) {
    if (strcmp(argv[1], "--sandbox") == 0) {
      sandbox = 1;
    } else if (strcmp(argv[1], "--decl") == 0 && argc > 3 &&
	       ndecls < (int)(sizeof(decls) / sizeof(decls[0]))) {
      decls[ndecls++] = argv[2];
      argv++;
      argc--;
    } else {
      break;
    }
    argv++;
    argc--;
  }
  if (argc != 2) {
    fprintf(stderr, "usage: nlrun [--sandbox] [--decl PATH]... FILE.nl\n");
    return 2;
  }
  NL_VERSION_SYM();
  L = luaL_newstate();
  lua_pushcfunction(L, openlibs);
  if (lua_pcall(L, 0, 0, 0) != 0) {
    print_error(L, "luaL_openlibs");
    return 1;
  }
  nl_settimer(L, timer, NULL);
  for (i = 0; i < ndecls; i++)
    if (!add_decl(L, decls[i])) return 2;
  if (getenv("NL_CONFORM_WORKERS") && *getenv("NL_CONFORM_WORKERS"))
    nl_setworkers(L, atoi(getenv("NL_CONFORM_WORKERS")));
  /* NL_CONFORM_JIT=hot: force the JIT (hotloop=1). */
  if (getenv("NL_CONFORM_JIT") && strcmp(getenv("NL_CONFORM_JIT"), "hot") == 0 &&
      luaL_dostring(L, "jit.opt.start('hotloop=1')") != 0) {
    print_error(L, "jit.opt.start");
    return 1;
  }
  if (sandbox && !nl_setoption(L, NL_OPT_SANDBOX, 1)) {
    fprintf(stderr, "nlrun: cannot set NL_OPT_SANDBOX\n");
    return 1;
  }
  if (luaL_loadfile(L, argv[1]) != 0) {
    print_error(L, argv[1]);
    return 1;
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
  for (;;) {
    if (nl_pump(L) > 0) continue;
    if (fire_timer(L)) continue;
    if (nl_drain(L) > 0) continue;
    break;
  }
  n = pending_tasks(L);
  fflush(stdout);
  if (n != 0) {
    fprintf(stderr, "nlrun: %d task(s) still pending and nothing left to run\n", n);
    lua_close(L);
    return 1;
  }
  lua_close(L);
  return 0;
}
