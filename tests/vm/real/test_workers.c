/*
** Workers with the real compiler and runtime: pool sizes 0, 1 and N, nl_spawn,
** nl_drain, nl_setworkerinit (also one that fails), a job that raises, a
** result that cannot cross, Task.sleep and nested spawns in a job,
** dedicated workers (calls, events both ways, stop with calls waiting, a
** file that fails to load or exports `stop`), NL_OPT_MAXWORKERS and
** NL_OPT_WORKERMEM, the wakeup callback, lua_close with workers busy and
** with messages queued, and a stress test that tests/vm/run.sh also runs
** under ThreadSanitizer.
**
**   test_workers DIR            the tests, in the scratch directory DIR
**   test_workers DIR --bench    also the measurements
**   test_workers DIR --stress   the stress test alone (ThreadSanitizer)
*/

#include <pthread.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <time.h>
#include <unistd.h>
#include <sys/stat.h>

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
/* Bytes the C heap has in use, for the leak checks of the churn tests (the
** sanitizer builds run without LeakSanitizer).
*/
static long heap_in_use(void)
{
  malloc_statistics_t st;
  malloc_zone_statistics(NULL, &st);
  return (long)st.size_in_use;
}
#define HAVE_HEAP_IN_USE 1
#endif

static void writefile(const char *name, const char *text)
{
  FILE *fp = fopen(name, "wb");
  if (!fp) { perror(name); exit(2); }
  fputs(text, fp);
  fclose(fp);
}

static double now(void)
{
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return (double)ts.tv_sec + (double)ts.tv_nsec * 1e-9;
}

static const char *JOBS =
  "# Jobs for test_workers.c. No top-level effects.\n"
  "pub struct Oops : Exception\n"
  "  code: Number\n"
  "end\n"
  "\n"
  "pub fn square(x)\n"
  "  x * x\n"
  "end\n"
  "\n"
  "pub fn fail(msg)\n"
  "  raise Oops { message: msg, code: 3 }\n"
  "end\n"
  "\n"
  "pub fn bad_result\n"
  "  let n = 1\n"
  "  let f = fn(x) x + n end\n"
  "  {f: f!}\n"
  "end\n"
  "\n"
  "pub fn double_on_host(x)\n"
  "  :host_double(x)\n"
  "end\n"
  "\n"
  "pub fn init_ran\n"
  "  \"#{:init_isworker}\"\n"
  "end\n"
  "\n"
  "pub fn sleepy(t)\n"
  "  Task.sleep(t).await\n"
  "  \"slept\"\n"
  "end\n"
  "\n"
  "pub fn nested(x)\n"
  "  let t = spawn square(x)\n"
  "  t.await + 1\n"
  "end\n"
  "\n"
  "pub fn deep(n)\n"
  "  return 0 if n == 0\n"
  "  (spawn deep(n - 1)).await + 1\n"
  "end\n"
  "\n"
  "pub fn spin(seconds)\n"
  "  let t0 = :os.clock()\n"
  "  while :os.clock() - t0 < seconds\n"
  "  end\n"
  "  \"spun\"\n"
  "end\n"
  "\n"
  "pub fn allocate(n)\n"
  "  let t = []\n"
  "  for i in 1..n\n"
  "    t[i] = \"a string of some length, number #{i}\"\n"
  "  end\n"
  "  t.len\n"
  "end\n"
  "\n"
  "pub fn tell_parent(v)\n"
  "  Worker.parent.emit \"FromJob\", v\n"
  "  v\n"
  "end\n"
  "\n"
  "pub fn count\n"
  "  Worker.count\n"
  "end\n"
  "\n"
  "pub fn memory\n"
  "  :collectgarbage(\"count\")\n"
  "end\n"
  "\n"
  "pub fn add(a, b)\n"
  "  a + b\n"
  "end\n"
  "\n"
  "pub fn start_svc\n"
  "  let p: any = 'svc'\n"
  "  Worker.new(p)\n"
  "end\n"
  "\n"
  "pub async fn svc_then_blow\n"
  "  let p: any = 'svc'\n"
  "  let w: any = Worker.new(p)\n"
  "  w.add(1).await\n"
  "  let pending = w.slow(5)\n"
  "  allocate(400000)\n"
  "  pending.await\n"
  "end\n";

static const char *SVC =
  "# A dedicated worker for test_workers.c.\n"
  "let total = 0\n"
  "\n"
  "pub fn add(n)\n"
  "  total += n\n"
  "  total\n"
  "end\n"
  "\n"
  "pub fn slow(t)\n"
  "  Task.sleep(t).await\n"
  "  \"slow done\"\n"
  "end\n"
  "\n"
  "pub fn square_there(x)\n"
  "  (spawn square(x)).await\n"
  "end\n"
  "\n"
  "on Ping do |x|\n"
  "  Worker.parent.emit \"Pong\", x * 2\n"
  "end\n";

static const char *SVC_IMPORT = "import { square } from 'jobs'\n";

static const char *EXPORTS_STOP =
  "pub fn stop\n"
  "  1\n"
  "end\n";

static const char *BROKEN = "pub fn f(\n";

static const char *LOADS_SPAWN =
  "import { square } from 'jobs'\n"
  "let t = spawn square(1)\n"
  "pub fn noop\n"
  "  1\n"
  "end\n";

/* -- States ---------------------------------------------------------------- */

static int host_double(lua_State *L)
{
  lua_pushnumber(L, 2 * luaL_checknumber(L, 1));
  return 1;
}

static int init_calls;
static pthread_mutex_t init_mtx = PTHREAD_MUTEX_INITIALIZER;

/* The worker-init function: the host API a worker may use. */
static int worker_init(lua_State *W)
{
  const char *tag = (const char *)lua_touserdata(W, 1);
  pthread_mutex_lock(&init_mtx);
  init_calls++;
  pthread_mutex_unlock(&init_mtx);
  lua_register(W, "host_double", host_double);
  lua_pushboolean(W, nl_isworker(W));
  lua_setglobal(W, "init_isworker");
  (void)tag;
  return 0;
}

static int worker_init_fails(lua_State *W)
{
  return luaL_error(W, "init broke");
}

static int openlibs(lua_State *L)
{
  luaL_openlibs(L);
  return 0;
}

/* The unhandled hook: counts. The tests read failures through the tasks,
** and a task they read is not awaited by NL code.
*/
static int unhandled_count;
static int on_unhandled(lua_State *L)
{
  (void)L;
  unhandled_count++;
  return 0;
}

static lua_State *newstate(int workers)
{
  lua_State *L = luaL_newstate();
  lua_pushcfunction(L, openlibs);
  if (lua_pcall(L, 0, 0, 0) != 0) {
    fprintf(stderr, "openlibs: %s\n", lua_tostring(L, -1));
    exit(1);
  }
  lua_pushcfunction(L, on_unhandled);
  nl_setunhandled(L);
  if (workers >= 0) nl_setworkers(L, workers);
  nl_setworkerinit(L, worker_init, (void *)"init");
  return L;
}

static const char *errtext(lua_State *L)
{
  if (lua_type(L, -1) == LUA_TSTRING) return lua_tostring(L, -1);
  return nlL_tolstring(L, -1, NULL);
}

static char result[4096];

/* Loads src (which defines `pub async fn run`), calls run() and drains
** until its task finishes; result is its value, or "<Type>: <message>" when
** it failed.
*/
static const char *run_main(lua_State *L, const char *src)
{
  int st, tries;
  lua_settop(L, 0);
  st = nl_loadbuffer(L, src, strlen(src), "=main");
  if (st == 0) st = nl_pcall(L, 0, 1);
  if (st != 0) {
    snprintf(result, sizeof(result), "load: %s", errtext(L));
    lua_settop(L, 0);
    return result;
  }
  lua_getfield(L, -1, "run");
  if (nl_pcall(L, 0, 1) != 0) {
    const char *t = nl_typeof(L, -1);
    lua_getfield(L, -1, "message");
    snprintf(result, sizeof(result), "%s: %s", t ? t : "?",
	     lua_isstring(L, -1) ? lua_tostring(L, -1) : "?");
    lua_settop(L, 0);
    return result;
  }
  for (tries = 0; tries < 100000; tries++) {
    st = nl_taskstate(L, -1);
    if (st != NL_TASK_PENDING) break;
    if (nl_drain(L) == 0 && nl_pump(L) == 0) {
      st = nl_taskstate(L, -1);
      if (st == NL_TASK_PENDING) { usleep(1000); }
    }
  }
  if (st == NL_TASK_DONE) {
    snprintf(result, sizeof(result), "%s", lua_isstring(L, -1) ? lua_tostring(L, -1) : "(not a string)");
  } else if (st == NL_TASK_FAILED) {
    const char *t = nl_typeof(L, -1);
    lua_getfield(L, -1, "message");
    snprintf(result, sizeof(result), "%s: %s", t ? t : "?",
	     lua_isstring(L, -1) ? lua_tostring(L, -1) : "?");
  } else {
    snprintf(result, sizeof(result), "still pending");
  }
  lua_settop(L, 0);
  return result;
}

/* -- The basics, with 0, 1 and N pool workers --------------------------------- */

static const char *BASICS =
  "import { square, fail, bad_result, double_on_host, init_ran, sleepy, nested, deep, Oops, count, tell_parent, add } from 'jobs'\n"
  "let heard = []\n"
  "on FromJob do |v|\n"
  "  heard[heard.len + 1] = v\n"
  "end\n"
  "pub async fn run\n"
  "  let out = []\n"
  "  out[out.len + 1] = spawn square(7).await\n"
  "  let tasks = []\n"
  "  for i in 1..50\n"
  "    tasks[i] = spawn square(i)\n"
  "  end\n"
  "  let sum = 0\n"
  "  for v in Task.all(tasks).await\n"
  "    sum += v\n"
  "  end\n"
  "  out[out.len + 1] = sum\n"
  "  try\n"
  "    spawn fail(\"bad\").await\n"
  "  catch e: Oops\n"
  "    out[out.len + 1] = \"#{typeof(e)} #{e.message} #{e.code} #{:string.find(e.trace, \"job started at\", 1, true) != nil}\"\n"
  "  end\n"
  "  try\n"
  "    spawn bad_result().await\n"
  "  catch e: SendError\n"
  "    out[out.len + 1] = e.message\n"
  "  end\n"
  "  out[out.len + 1] = spawn double_on_host(21).await\n"
  "  out[out.len + 1] = spawn init_ran().await\n"
  "  out[out.len + 1] = spawn sleepy(0.02).await\n"
  "  out[out.len + 1] = spawn nested(5).await\n"
  "  out[out.len + 1] = spawn deep(6).await\n"
  "  out[out.len + 1] = spawn tell_parent(9).await\n"
  "  out[out.len + 1] = heard[1]\n"
  "  out[out.len + 1] = Table.concat(Worker.map([1, 2, 3, 4, 5], add!, 10).await, \",\")\n"
  "  out[out.len + 1] = \"#{spawn count().await == Worker.count}\"\n"
  "  out[out.len + 1] = \"#{Worker.parent == nil}\"\n"
  "  Table.concat(out, \"|\")\n"
  "end\n";

static const char *BASICS_OUT =
  "49|42925|Oops bad 3 true|cannot send a function without a name (result, at .f)|42|true|slept|26|6|9|9|11,12,13,14,15|true|true";

static void basics(int workers)
{
  lua_State *L = newstate(workers);
  double t0 = now();
  CHECK_STR(run_main(L, BASICS), BASICS_OUT);
  printf("# basics with %d pool workers: %.1f ms\n", workers, (now() - t0) * 1e3);
  lua_close(L);
}

static void test_pool_zero(void) { basics(0); }
static void test_pool_one(void) { basics(1); }
static void test_pool_four(void) { basics(4); }

static void test_count(void)
{
  lua_State *L = newstate(-1);
  long cores = sysconf(_SC_NPROCESSORS_ONLN);
  char want[32];
  snprintf(want, sizeof(want), "%ld", cores > 2 ? cores - 1 : 1);
  CHECK_STR(run_main(L, "pub async fn run\n  \"#{Worker.count}\"\nend\n"), want);
  nl_setworkers(L, 3);
  CHECK_STR(run_main(L, "pub async fn run\n  \"#{Worker.count}\"\nend\n"), "3");
  lua_close(L);
  L = newstate(0);
  CHECK_STR(run_main(L, "pub async fn run\n  \"#{Worker.count}\"\nend\n"), "0");
  CHECK(nl_isworker(L) == 0);
  lua_close(L);
}

/* -- nl_spawn and nl_drain ------------------------------------------------------ */

static int spawn_c(lua_State *S)
{
  nl_spawn(S, lua_gettop(S) - 1);
  return 1;
}

static void test_nl_spawn(void)
{
  lua_State *L = newstate(2);
  int n;
  nl_import(L, "jobs");
  lua_getfield(L, -1, "square");
  lua_pushnumber(L, 12);
  nl_spawn(L, 1);
  CHECK(nl_taskstate(L, -1) == NL_TASK_PENDING);
  n = nl_drain(L);
  CHECK(n > 0);
  CHECK(nl_taskstate(L, -1) == NL_TASK_DONE);
  CHECK(lua_tonumber(L, -1) == 144);
  lua_settop(L, 0);
  /* A C function has no name: SendError, at once. */
  lua_pushcfunction(L, spawn_c);
  lua_pushcfunction(L, host_double);
  lua_pushnumber(L, 1);
  CHECK(nl_pcall(L, 2, 1) != 0);
  CHECK_STR(nl_typeof(L, -1), "SendError");
  lua_getfield(L, -1, "message");
  CHECK_STR(lua_tostring(L, -1), "cannot send a function without a name (argument 1)");
  lua_settop(L, 0);
  /* Nothing outstanding: nl_drain returns at once. */
  CHECK(nl_drain(L) == 0);
  lua_close(L);
}

/* -- The worker-init function --------------------------------------------------- */

static void test_init_fails(void)
{
  lua_State *L = newstate(1);
  nl_setworkerinit(L, worker_init_fails, NULL);
  CHECK_STR(run_main(L, "import { square } from 'jobs'\npub async fn run\n  spawn square(2).await\nend\n"),
	    "WorkerError: the worker-init function failed: init broke");
  CHECK_STR(run_main(L, "pub async fn run\n  let w = Worker.new 'svc'\n  w.add(1).await\nend\n"),
	    "WorkerError: the worker-init function failed: init broke");
  lua_close(L);
  L = newstate(0);
  nl_setworkerinit(L, worker_init_fails, NULL);
  CHECK_STR(run_main(L, "import { square } from 'jobs'\npub async fn run\n  spawn square(2).await\nend\n"),
	    "WorkerError: the worker-init function failed: init broke");
  lua_close(L);
}

/* -- Loading a file in a pool worker -------------------------------------------- */

static void test_spawn_while_loading(void)
{
  /* The main state runs the file's top-level spawn as any other code; a
  ** pool worker that loads the file for a job may not, so the
  ** load fails, and the job with it.
  */
  lua_State *L = newstate(1);
  CHECK_CONTAINS(run_main(L, "import { noop } from 'loads_spawn'\npub async fn run\n  \"#{spawn noop().await}\"\nend\n"),
		 "WorkerError: A spawn cannot run while a pool worker is loading a file for a job");
  lua_close(L);
}

/* -- Dedicated workers ---------------------------------------------------------- */

static const char *DEDICATED =
  "let pongs = []\n"
  "on Pong do |v|\n"
  "  pongs[pongs.len + 1] = v\n"
  "end\n"
  "pub async fn run\n"
  "  let out = []\n"
  "  let svc = Worker.new 'svc'\n"
  "  out[out.len + 1] = svc.add(5).await\n"
  "  out[out.len + 1] = svc.add(3).await\n"
  "  svc.emit \"Ping\", 21\n"
  "  out[out.len + 1] = svc.add(0).await\n"
  "  out[out.len + 1] = pongs[1]\n"
  "  out[out.len + 1] = svc.square_there(9).await\n"
  "  let name: any = 'nothing_here'\n"
  "  try\n"
  "    svc[name](svc, 1).await\n"
  "  catch e: LuaError\n"
  "    out[out.len + 1] = typeof(e)\n"
  "  end\n"
  "  let first = svc.add(1)\n"
  "  let slow = svc.slow(0.2)\n"
  "  svc.stop\n"
  "  let after = svc.add(1)\n"
  "  out[out.len + 1] = first.await\n"
  "  try\n"
  "    slow.await\n"
  "  catch e: WorkerError\n"
  "    out[out.len + 1] = \"slow: WorkerError\"\n"
  "  end\n"
  "  try\n"
  "    after.await\n"
  "  catch e: WorkerError\n"
  "    out[out.len + 1] = \"after: WorkerError\"\n"
  "  end\n"
  "  try\n"
  "    svc.add(1).await\n"
  "  catch e: WorkerError\n"
  "    out[out.len + 1] = \"later: WorkerError\"\n"
  "  end\n"
  "  let path: any = 'exports_stop'\n"
  "  let bad: any = Worker.new(path)\n"
  "  try\n"
  "    bad.stop_it().await\n"
  "  catch e: WorkerError\n"
  "    out[out.len + 1] = \"exports stop: WorkerError\"\n"
  "  end\n"
  "  path = 'broken'\n"
  "  let broken: any = Worker.new(path)\n"
  "  try\n"
  "    broken.f(1).await\n"
  "  catch e: LuaError\n"
  "    out[out.len + 1] = \"broken: #{typeof(e)} #{:string.find(e.message, \"broken\", 1, true) != nil}\"\n"
  "  end\n"
  "  path = 'no_such_file'\n"
  "  let missing: any = Worker.new(path)\n"
  "  try\n"
  "    missing.f(1).await\n"
  "  catch e\n"
  "    out[out.len + 1] = \"missing: #{typeof(e)}\"\n"
  "  end\n"
  "  Table.concat(out, \"|\")\n"
  "end\n";

static const char *DEDICATED_OUT =
  "5|8|8|42|81|LuaError|9|slow: WorkerError|after: WorkerError|later: WorkerError|"
  "exports stop: WorkerError|broken: LuaError true|missing: LuaError";

static void dedicated(int workers)
{
  lua_State *L = newstate(workers);
  CHECK_STR(run_main(L, DEDICATED), DEDICATED_OUT);
  lua_close(L);
}

static void test_dedicated_zero(void) { dedicated(0); }
static void test_dedicated_threads(void) { dedicated(2); }

static void test_maxworkers(void)
{
  lua_State *L = newstate(1);
  CHECK(nl_setoption(L, NL_OPT_MAXWORKERS, 2) == 1);
  CHECK_STR(run_main(L,
    "pub async fn run\n"
    "  let a = Worker.new 'svc'\n"
    "  let b = Worker.new 'svc'\n"
    "  try\n"
    "    let c = Worker.new 'svc'\n"
    "    \"three\"\n"
    "  catch e: WorkerError\n"
    "    a.stop\n"
    "    \"#{a.add(0) is Task} #{b.add(2).await} #{e.message}\"\n"
    "  end\n"
    "end\n"),
    "true 2 too many dedicated workers: NL_OPT_MAXWORKERS allows 2 at once");
  lua_close(L);
}

/* -- NL_OPT_WORKERMEM ------------------------------------------------------------ */

static double worker_kb;

static void test_workermem(void)
{
  lua_State *L = newstate(1);
  char src[512];
  const char *r;
  double base;
  r = run_main(L, "import { memory } from 'jobs'\npub async fn run\n  \"#{spawn memory().await}\"\nend\n");
  base = atof(r);
  worker_kb = base;
  CHECK(base > 0);
  lua_close(L);
  L = newstate(1);
  /* The limit: what a worker uses with the runtime and jobs.nl loaded, and
  ** 4 MB more.
  */
  CHECK(nl_setoption(L, NL_OPT_WORKERMEM, (lua_Integer)(base * 1024) + (4 << 20)) == 1);
  snprintf(src, sizeof(src),
    "import { allocate, square } from 'jobs'\n"
    "pub async fn run\n"
    "  let small = spawn allocate(100).await\n"
    "  let big = \"no error\"\n"
    "  try\n"
    "    spawn allocate(400000).await\n"
    "  catch e: WorkerError\n"
    "    big = \"WorkerError #{:string.find(e.message, \"NL_OPT_WORKERMEM\", 1, true) != nil}\"\n"
    "  end\n"
    "  \"#{small} #{big} #{spawn square(3).await}\"\n"
    "end\n");
  CHECK_STR(run_main(L, src), "100 WorkerError true 9");
  lua_close(L);
  /* A limit below what a worker needs to open its libraries: every job
  ** fails with WorkerError, and the worker is not replaced over and over.
  */
  L = newstate(1);
  CHECK(nl_setoption(L, NL_OPT_WORKERMEM, 64 * 1024) == 1);
  CHECK_CONTAINS(run_main(L, "import { square } from 'jobs'\npub async fn run\n  spawn square(2).await\nend\n"),
		 "too small to open its libraries");
  CHECK_CONTAINS(run_main(L, "import { square } from 'jobs'\npub async fn run\n  spawn square(3).await\nend\n"),
		 "WorkerError");
  lua_close(L);
}

/* A worker that opens under a limit too tight for its libraries, at many
** limits: every allocation that fails while the libraries and the runtime
** open must leave nothing behind (LuaJIT's assertion build checks that the
** state's memory is all freed when it closes).
*/
static void test_open_under_limit(void)
{
  int kb, failed = 0, done = 0;
  for (kb = 64; kb <= 480; kb += 2) {
    lua_State *L = newstate(0);
    const char *r;
    nl_setoption(L, NL_OPT_WORKERMEM, (lua_Integer)kb * 1024);
    r = run_main(L, "import { square } from 'jobs'\npub async fn run\n  \"#{spawn square(2).await}\"\nend\n");
    if (strcmp(r, "4") == 0) done++;
    else if (strstr(r, "WorkerError")) failed++;
    else { fprintf(stderr, "  limit %d KB: %s\n", kb, r); CHECK(0); }
    lua_close(L);
  }
  printf("# open under a limit, 64 to 480 KB: %d failed with WorkerError, %d ran\n", failed, done);
  CHECK(failed > 0);
}

/* -- The sandbox, and settings fixed once workers exist ---------------------------- */

static const char *HOSTJOBS =
  "pub fn sq(x)\n  x * x\nend\n"
  "pub fn host_io\n  \"#{:io != nil}\"\nend\n";
static const char *MOD =
  "pub fn evil\n  \"io #{:io != nil} os #{:os != nil}\"\nend\n"
  "pub fn here\n  \"io #{:io != nil}\"\nend\n";

/* The value of the task on top after draining: its string, or the
** exception's type and message.
*/
static const char *drained(lua_State *L)
{
  int st;
  nl_drain(L);
  st = nl_taskstate(L, -1);
  if (st == NL_TASK_DONE) {
    snprintf(result, sizeof(result), "%s", lua_isstring(L, -1) ? lua_tostring(L, -1) : "(not a string)");
  } else if (st == NL_TASK_FAILED) {
    const char *t = nl_typeof(L, -1);
    lua_getfield(L, -1, "message");
    snprintf(result, sizeof(result), "%s: %s", t ? t : "?", lua_isstring(L, -1) ? lua_tostring(L, -1) : "?");
  } else {
    snprintf(result, sizeof(result), "state %d", st);
  }
  lua_settop(L, 0);
  return result;
}

static void spawn_export(lua_State *L, const char *file, const char *fn)
{
  lua_settop(L, 0);
  nl_import(L, file);
  lua_getfield(L, -1, fn);
  lua_remove(L, -2);
  nl_spawn(L, 0);
}

static void test_sandbox_after_spawn(void)
{
  int mode;
  for (mode = 0; mode < 2; mode++) {
    lua_State *L = newstate(mode);  /* 0 workers, then 1 */
    /* The host's own job runs before the sandbox: the pool exists. */
    nl_import(L, "hostjobs");
    lua_getfield(L, -1, "sq");
    lua_pushnumber(L, 3);
    nl_spawn(L, 1);
    CHECK_STR(drained(L), "9");
    CHECK(nl_setoption(L, NL_OPT_SANDBOX, 1) == 1);
    /* The settings are fixed now; the sandbox could still be turned on. */
    CHECK(nl_setoption(L, NL_OPT_CHECKS, 0) == 0);
    CHECK(nl_setoption(L, NL_OPT_WORKERMEM, 1 << 20) == 0);
    CHECK(nl_getoption(L, NL_OPT_CHECKS) == 1);
    /* An untrusted mod loaded after the sandbox: the same in a worker as
    ** in the main state.
    */
    lua_settop(L, 0);
    nl_import(L, "mod");
    lua_getfield(L, -1, "here");
    CHECK(nl_pcall(L, 0, 1) == 0);
    CHECK_STR(lua_tostring(L, -1), "io false");
    spawn_export(L, "mod", "evil");
    CHECK_STR(drained(L), "io false os false");
    /* A host file the worker had loaded before keeps what it had, as in
    ** the main state.
    */
    spawn_export(L, "hostjobs", "host_io");
    CHECK_STR(drained(L), "true");
    lua_close(L);
  }
  /* A dedicated worker that exists before the sandbox gets it too. */
  {
    lua_State *L = newstate(1);
    const char *src = "pub fn mk\n  Worker.new 'modsvc'\nend\npub async fn ask(w: any)\n  w.evil().await\nend\n";
    int st = nl_loadbuffer(L, src, strlen(src), "=svcmain");
    if (st == 0) st = nl_pcall(L, 0, 1);
    CHECK(st == 0);
    lua_getfield(L, -1, "mk");
    CHECK(nl_pcall(L, 0, 1) == 0);  /* exports, handle */
    nl_drain(L);
    CHECK(nl_setoption(L, NL_OPT_SANDBOX, 1) == 1);
    lua_getfield(L, 1, "ask");
    lua_pushvalue(L, 2);
    CHECK(nl_pcall(L, 1, 1) == 0);
    CHECK_STR(drained(L), "io false os false");
    lua_close(L);
  }
  /* Setters that cannot fail have no effect once workers exist. */
  {
    lua_State *L = newstate(1);
    nl_import(L, "jobs");
    lua_getfield(L, -1, "square");
    lua_pushnumber(L, 2);
    nl_spawn(L, 1);
    CHECK_STR(drained(L), "4");
    nl_setworkers(L, 5);
    CHECK_STR(run_main(L, "pub async fn run\n  \"#{Worker.count}\"\nend\n"), "1");
    lua_close(L);
  }
}

/* -- Worker.parent, an init function over the limit, a discarded parent ----------- */

static int big_init(lua_State *W)
{
  pthread_mutex_lock(&init_mtx);
  init_calls++;
  pthread_mutex_unlock(&init_mtx);
  lua_createtable(W, 400000, 0);  /* about 6 MB */
  lua_setglobal(W, "big");
  return 0;
}

static void test_init_over_limit(void)
{
  int workers;
  for (workers = 0; workers < 2; workers++) {
    lua_State *L = newstate(workers);
    int before, after, i;
    nl_setworkerinit(L, big_init, NULL);
    CHECK(nl_setoption(L, NL_OPT_WORKERMEM, 2 << 20) == 1);
    CHECK_CONTAINS(run_main(L, "import { square } from 'jobs'\npub async fn run\n  spawn square(2).await\nend\n"),
		   "WorkerError: the memory limit of a worker");
    /* The worker is not replaced over and over while idle. */
    pthread_mutex_lock(&init_mtx);
    before = init_calls;
    pthread_mutex_unlock(&init_mtx);
    for (i = 0; i < 20; i++) { nl_pump(L); usleep(5000); }
    pthread_mutex_lock(&init_mtx);
    after = init_calls;
    pthread_mutex_unlock(&init_mtx);
    CHECK(after == before);
    CHECK_CONTAINS(run_main(L, "import { square } from 'jobs'\npub async fn run\n  spawn square(3).await\nend\n"),
		   "WorkerError");
    lua_close(L);
  }
}

static void test_parent_of_subtasks(void)
{
  /* A task that job A starts emits to A's state, also while a job of
  ** another state runs on the same pool worker.
  */
  lua_State *L = newstate(1);
  CHECK_STR(run_main(L,
    "import { job_a } from 'pj'\n"
    "let heard = []\n"
    "on Hello do |who|\n  heard[heard.len + 1] = \"main heard #{who}\"\nend\n"
    "pub async fn run\n"
    "  let d = Worker.new 'dsvc'\n"
    "  let a = spawn job_a()\n"
    "  let b = d.start_b()\n"
    "  a.await\n"
    "  b.await\n"
    "  let dh = d.heard().await\n"
    "  \"#{Table.concat(heard, \",\")} | #{dh}\"\n"
    "end\n"),
    "main heard from job A's sub-task | ");
  lua_close(L);
}

static void test_discarded_parent(void)
{
  /* A pool job starts a dedicated worker, then its pool worker is
  ** discarded: the dedicated worker stops with it (its state closed), and
  ** a call to it fails with WorkerError instead of waiting forever.
  */
  lua_State *L = newstate(1);
  CHECK(nl_setoption(L, NL_OPT_WORKERMEM, 8 << 20) == 1);
  CHECK_STR(run_main(L,
    "import { start_svc, allocate } from 'jobs'\n"
    "pub async fn run\n"
    "  let w: any = spawn start_svc().await\n"
    "  \"started\"\n"
    "end\n"),
    "SendError: cannot send a table with a metatable that is not a struct (result)");
  CHECK_STR(run_main(L,
    "import { svc_then_blow } from 'jobs'\n"
    "pub async fn run\n"
    "  try\n"
    "    spawn svc_then_blow().await\n"
    "    \"no error\"\n"
    "  catch e: WorkerError\n"
    "    \"WorkerError\"\n"
    "  end\n"
    "end\n"),
    "WorkerError");
  lua_close(L);
}

/* -- The search of a worker, Worker.parent, a pool that dies ---------------- */

/* The init function widens the worker's own search. */
static int init_searchok;
static int init_addpath(lua_State *W)
{
  init_searchok = nl_isworker(W) && nl_workersstarted(W);
  nl_addpath(W, "extra");
  return 0;
}

static int host_seven(lua_State *L) { lua_pushnumber(L, 7); return 1; }

/* An init function that offers host_seven to sandboxed code too. */
static int init_seven(lua_State *W)
{
  lua_register(W, "host_seven", host_seven);
  nl_sandboxenv(W);
  lua_pushcfunction(W, host_seven);
  lua_setfield(W, -2, "host_seven");
  lua_pop(W, 1);
  return 0;
}

static void spawn_export_n(lua_State *L, const char *file, const char *fn, int nargs, double arg)
{
  lua_settop(L, 0);
  nl_import(L, file);
  lua_getfield(L, -1, fn);
  lua_remove(L, -2);
  if (nargs) lua_pushnumber(L, arg);
  nl_spawn(L, nargs);
}

static void test_worker_search(void)
{
  int mode;
  /* nl_addpath in the init function: the worker finds what the main state
  ** cannot; on the main state, once workers exist, nl_addpath has no effect,
  ** which nl_workersstarted tells.
  */
  {
    lua_State *L = newstate(1);
    CHECK(nl_workersstarted(L) == 0);
    nl_setworkerinit(L, init_addpath, NULL);
    CHECK_STR(run_main(L, "pub async fn run\n  let p = \"wsearch#{1}\"\n  let w = Worker.new p\n  w.run_h().await\nend\n"),
	      "helper found");
    CHECK(init_searchok == 1);
    CHECK(nl_workersstarted(L) == 1 && nl_isworker(L) == 0);
    nl_addpath(L, "late");
    CHECK_CONTAINS(run_main(L, "import { l } from 'latemod'\npub async fn run\n  l()\nend\n"), "load:");
    lua_close(L);
  }
  /* The declaration files of the search path, for a worker that starts
  ** after the sandbox went on, as for one that existed: mode 0
  ** never sandboxed, 1 sandboxed after the pool started, 2 before.
  */
  for (mode = 0; mode < 3; mode++) {
    lua_State *L = newstate(1);
    nl_setworkerinit(L, init_seven, NULL);
    nl_addpath(L, "decls");
    if (mode == 2) CHECK(nl_setoption(L, NL_OPT_SANDBOX, 1) == 1);
    spawn_export_n(L, "jobs7", "sq", 1, 3);
    CHECK_STR(drained(L), "9");
    if (mode == 1) CHECK(nl_setoption(L, NL_OPT_SANDBOX, 1) == 1);
    spawn_export_n(L, "jobs7", "seven", 0, 0);
    CHECK_STR(drained(L), "7");
    CHECK_STR(run_main(L, "pub async fn run\n  let w = Worker.new 'svc7'\n  \"dedicated: #{w.seven().await}\"\nend\n"),
	      "dedicated: 7");
    lua_close(L);
  }
}

static void test_parent_in_callbacks(void)
{
  int workers;
  /* A coroutine that job A makes, and an on_done callback of a task that
  ** job A starts: Worker.parent names A's state, also while a job of
  ** another state runs on the same pool worker.
  */
  const char *files[2] = { "pjco", "pjdone" };
  const char *heard[2] = { "from job A's coroutine", "from job A's on_done" };
  int i;
  for (i = 0; i < 2; i++) {
    lua_State *L = newstate(i == 0 ? 1 : 2);
    char src[1024], want[256];
    snprintf(src, sizeof(src),
      "import { job_a } from '%s'\n"
      "let heard = []\n"
      "on Hello do |who|\n  heard[heard.len + 1] = \"main heard #{who}\"\nend\n"
      "pub async fn run\n"
      "  let d = Worker.new 'dsvc'\n"
      "  let a = spawn job_a()\n"
      "  let b = d.start_b()\n"
      "  a.await\n"
      "  b.await\n"
      "  let dh = d.heard().await\n"
      "  \"main: [#{Table.concat(heard, \",\")}] dedicated: [#{dh}]\"\n"
      "end\n", files[i]);
    snprintf(want, sizeof(want), "main: [main heard %s] dedicated: []", heard[i]);
    CHECK_STR(run_main(L, src), want);
    lua_close(L);
  }
  /* A job that a pool worker spawned for itself and that runs there: its
  ** Worker.parent would name the worker itself, where an event would
  ** vanish; it is a WorkerError instead.
  */
  for (workers = 0; workers < 2; workers++) {
    lua_State *L = newstate(workers);
    CHECK_CONTAINS(run_main(L, "import { outer } from 'selfp'\npub async fn run\n  spawn outer().await\nend\n"),
		   "WorkerError: Worker.parent names this worker itself");
    lua_close(L);
  }
}

static void test_parent_no_job(void)
{
  /* A timer's callback outside any task runs for no job: Worker.parent is
  ** nil there, as in the main state. The callback of a call the job made to
  ** a dedicated worker runs for the job, as the file the result needs
  ** loads for it (its `main() unless Worker.parent` stays quiet).
  */
  {
    lua_State *L = newstate(1);
    CHECK_STR(run_main(L, "import { job_a } from 'pjnone'\npub async fn run\n  spawn job_a().await\nend\n"),
	      "callback: nil, call's callback: a handle, job: a handle");
    lua_close(L);
  }
  /* A file that a pool worker loads for no job (for the result of a spawn
  ** made by such a callback, which brings a struct the worker has not
  ** loaded): Worker.parent is a WorkerError there, so that the file's
  ** `main() unless Worker.parent` does not run. The callback keeps its
  ** worker busy, so that the other pool worker runs the spawned job.
  */
  {
    lua_State *L = newstate(2);
    CHECK_STR(run_main(L, "import { job_a } from 'pjload'\npub async fn run\n  spawn job_a().await\nend\n"),
	      "WorkerError: Worker.parent is unknown here: this pool worker is loading a file for code that runs "
	      "for no job");
    lua_close(L);
  }
}

/* For tests only (lj_nlworker.c): how many more worker states may be made. */
extern int lj_nl_test_workerstates;

static void test_pool_dies(void)
{
  /* No pool worker can start: a spawn fails with WorkerError instead of
  ** waiting in a queue that no one serves.
  */
  {
    lua_State *L = newstate(1);
    lj_nl_test_workerstates = 0;
    CHECK_CONTAINS(run_main(L, "import { square } from 'jobs'\npub async fn run\n  spawn square(2).await\nend\n"),
		   "WorkerError");
    CHECK_CONTAINS(run_main(L, "import { square } from 'jobs'\npub async fn run\n  spawn square(3).await\nend\n"),
		   "WorkerError");
    lj_nl_test_workerstates = -1;
    lua_close(L);
  }
  /* The one pool worker is discarded over its memory limit and cannot be
  ** replaced: the next spawn fails the same way.
  */
  {
    lua_State *L = newstate(1);
    char src[512];
    if (worker_kb <= 0) {  /* test_workermem measures it; TW_ONLY may skip that */
      worker_kb = atof(run_main(L, "import { memory } from 'jobs'\npub async fn run\n  \"#{spawn memory().await}\"\nend\n"));
      lua_close(L);
      L = newstate(1);
    }
    CHECK(worker_kb > 0);
    CHECK(nl_setoption(L, NL_OPT_WORKERMEM, (lua_Integer)(worker_kb * 1024) + (4 << 20)) == 1);
    lj_nl_test_workerstates = 1;
    snprintf(src, sizeof(src),
      "import { allocate, square } from 'jobs'\n"
      "pub async fn run\n"
      "  let small = spawn allocate(100).await\n"
      "  let big = \"no error\"\n"
      "  try\n"
      "    spawn allocate(400000).await\n"
      "  catch e: WorkerError\n"
      "    big = \"WorkerError\"\n"
      "  end\n"
      "  \"#{small} #{big}\"\n"
      "end\n");
    CHECK_STR(run_main(L, src), "100 WorkerError");
    CHECK_CONTAINS(run_main(L, "import { square } from 'jobs'\npub async fn run\n  spawn square(3).await\nend\n"),
		   "WorkerError: no pool worker is");
    /* By now no pool worker is alive for certain: the spawn is refused
    ** where it would be queued.
    */
    CHECK_CONTAINS(run_main(L, "import { square } from 'jobs'\npub async fn run\n  spawn square(4).await\nend\n"),
		   "WorkerError: no pool worker is alive");
    lj_nl_test_workerstates = -1;
    lua_close(L);
  }
}

/* -- Dedicated workers that come and go ---------------------------------------------- */

static void test_churn(void)
{
  lua_State *L = newstate(2);
  const char *src =
    "pub async fn round(n)\n"
    "  for i in 1..n\n"
    "    let w = Worker.new 'svc'\n"
    "    w.add(1).await\n"
    "    w.stop\n"
    "  end\n"
    "  \"ok\"\n"
    "end\n"
    "pub fn keep\n  Worker.new 'svc'\nend\n"
    "pub async fn ask(w: any)\n  w.add(1).await\nend\n";
  int st = nl_loadbuffer(L, src, strlen(src), "=churn");
  int round, ex;
#ifdef HAVE_HEAP_IN_USE
  long base = 0, grown = 0;
#endif
  if (st == 0) st = nl_pcall(L, 0, 1);
  CHECK(st == 0);
  if (st != 0) { lua_close(L); return; }
  ex = luaL_ref(L, LUA_REGISTRYINDEX);  /* drained() empties the stack. */
  for (round = 0; round < 4; round++) {
    int i;
    lua_settop(L, 0);
    lua_rawgeti(L, LUA_REGISTRYINDEX, ex);
    lua_getfield(L, 1, "round");
    lua_pushnumber(L, 60);
    CHECK(nl_pcall(L, 1, 1) == 0);
    CHECK_STR(drained(L), "ok");
    lua_gc(L, LUA_GCCOLLECT, 0);
    for (i = 0; i < 50; i++) { nl_pump(L); usleep(2000); }
#ifdef HAVE_HEAP_IN_USE
    if (round == 1) base = heap_in_use();
    if (round == 3) grown = heap_in_use() - base;
#endif
  }
#ifdef HAVE_HEAP_IN_USE
  printf("# 240 dedicated workers started and stopped; C heap after rounds 2 to 4: %+ld bytes\n", grown);
  CHECK(grown < 64 * 1024);
#endif
  /* A handle to a worker that has ended: WorkerError, not freed memory. */
  lua_settop(L, 0);
  lua_rawgeti(L, LUA_REGISTRYINDEX, ex);
  lua_getfield(L, 1, "keep");
  CHECK(nl_pcall(L, 0, 1) == 0);  /* 2: the handle */
  lua_getfield(L, 2, "stop");
  lua_pushvalue(L, 2);
  CHECK(nl_pcall(L, 1, 0) == 0);
  {
    int i;
    for (i = 0; i < 50; i++) { nl_pump(L); usleep(2000); }
  }
  lua_getfield(L, 1, "ask");
  lua_pushvalue(L, 2);
  CHECK(nl_pcall(L, 1, 1) == 0);
  CHECK_CONTAINS(drained(L), "WorkerError");
  lua_close(L);
}

/* -- The wakeup callback ---------------------------------------------------------- */

static pthread_mutex_t wake_mtx = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t wake_cond = PTHREAD_COND_INITIALIZER;
static int wakeups;

static void on_wakeup(void *data)
{
  (void)data;
  pthread_mutex_lock(&wake_mtx);
  wakeups++;
  pthread_cond_broadcast(&wake_cond);
  pthread_mutex_unlock(&wake_mtx);
}

static void test_wakeup(void)
{
  lua_State *L = newstate(2);
  int got, i;
  nl_setwakeup(L, on_wakeup, NULL);
  nl_import(L, "jobs");
  lua_getfield(L, -1, "square");
  lua_pushnumber(L, 3);
  nl_spawn(L, 1);
  /* The host sleeps; the worker's result wakes it. */
  pthread_mutex_lock(&wake_mtx);
  for (i = 0; i < 500 && wakeups == 0; i++) {
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    ts.tv_nsec += 10 * 1000000;
    if (ts.tv_nsec >= 1000000000) { ts.tv_sec++; ts.tv_nsec -= 1000000000; }
    pthread_cond_timedwait(&wake_cond, &wake_mtx, &ts);
  }
  got = wakeups;
  pthread_mutex_unlock(&wake_mtx);
  CHECK(got > 0);
  CHECK(nl_pump(L) > 0);
  CHECK(nl_taskstate(L, -1) == NL_TASK_DONE);
  CHECK(lua_tonumber(L, -1) == 9);
  lua_settop(L, 0);
  nl_setwakeup(L, NULL, NULL);
  lua_close(L);
}

/* -- lua_close --------------------------------------------------------------------- */

static void test_close_busy(void)
{
  /* Workers busy, jobs still queued, results on their way: lua_close stops
  ** the workers, waits for them and frees everything.
  */
  int mode;
  for (mode = 0; mode < 3; mode++) {
    lua_State *L = newstate(mode == 0 ? 0 : mode == 1 ? 1 : 4);
    int st;
    st = nl_loadbuffer(L,
      "import { spin, square } from 'jobs'\n"
      "pub fn start\n"
      "  let tasks = []\n"
      "  for i in 1..4\n"
      "    tasks[tasks.len + 1] = spawn spin(0.05)\n"
      "  end\n"
      "  for i in 1..300\n"
      "    tasks[tasks.len + 1] = spawn square(i)\n"
      "  end\n"
      "  let svc = Worker.new 'svc'\n"
      "  for i in 1..100\n"
      "    tasks[tasks.len + 1] = svc.add(i)\n"
      "  end\n"
      "  tasks.len\n"
      "end\n", 0, "=close");
    {
      const char *src =
	"import { spin, square } from 'jobs'\n"
	"pub fn start\n"
	"  let tasks = []\n"
	"  for i in 1..4\n"
	"    tasks[tasks.len + 1] = spawn spin(0.05)\n"
	"  end\n"
	"  for i in 1..300\n"
	"    tasks[tasks.len + 1] = spawn square(i)\n"
	"  end\n"
	"  let svc = Worker.new 'svc'\n"
	"  for i in 1..100\n"
	"    tasks[tasks.len + 1] = svc.add(i)\n"
	"  end\n"
	"  tasks.len\n"
	"end\n";
      lua_settop(L, 0);
      st = nl_loadbuffer(L, src, strlen(src), "=close");
      if (st == 0) st = nl_pcall(L, 0, 1);
      CHECK(st == 0);
      if (st == 0) {
	lua_getfield(L, -1, "start");
	CHECK(nl_pcall(L, 0, 1) == 0);
	CHECK(lua_tonumber(L, -1) == 404);
      }
      if (mode == 2) {
	/* Let some results arrive in the inbox, unpumped. */
	usleep(20000);
      }
    }
    lua_close(L);
  }
}

/* -- Stress (also under ThreadSanitizer) --------------------------------------------- */

static int posted_runs;

static void posted(lua_State *L, void *data)
{
  (void)data;
  if (L) posted_runs++;
}

typedef struct Poster { lua_State *L; int n; } Poster;

static void *poster(void *arg)
{
  Poster *p = (Poster *)arg;
  int i;
  for (i = 0; i < p->n; i++) {
    nl_post(p->L, posted, NULL);
    if (i % 64 == 0) sched_yield();
  }
  return NULL;
}

static const char *STRESS =
  "import { square, nested, add, fail, Oops } from 'jobs'\n"
  "pub async fn run\n"
  "  let tasks = []\n"
  "  for i in 1..1500\n"
  "    if i % 3 == 0\n"
  "      tasks[i] = spawn nested(i)\n"
  "    else\n"
  "      tasks[i] = spawn square(i)\n"
  "    end\n"
  "  end\n"
  "  let svcs = []\n"
  "  for k in 1..4\n"
  "    svcs[k] = Worker.new 'svc'\n"
  "  end\n"
  "  let calls = []\n"
  "  for i in 1..400\n"
  "    calls[i] = svcs[(i % 4) + 1].add(1)\n"
  "  end\n"
  "  let failures = 0\n"
  "  for i in 1..50\n"
  "    try\n"
  "      spawn fail(\"x\").await\n"
  "    catch e: Oops\n"
  "      failures += 1\n"
  "    end\n"
  "  end\n"
  "  let sum = 0\n"
  "  for v in Task.all(tasks).await\n"
  "    sum += v\n"
  "  end\n"
  "  let top = 0\n"
  "  for v in Task.all(calls).await\n"
  "    top = v if v > top\n"
  "  end\n"
  "  let mapped = Worker.map([1, 2, 3, 4, 5, 6, 7, 8, 9, 10], add!, 1).await\n"
  "  for s in svcs\n"
  "    s.stop\n"
  "  end\n"
  "  \"#{sum} #{top} #{failures} #{mapped[10]}\"\n"
  "end\n";

static void stress(int workers)
{
  lua_State *L = newstate(workers);
  pthread_t th[4];
  Poster ps[4];
  long expect = 0;
  int i;
  char want[128];
  for (i = 1; i <= 1500; i++) expect += (i % 3 == 0) ? (long)i * i + 1 : (long)i * i;
  snprintf(want, sizeof(want), "%ld 100 50 11", expect);
  posted_runs = 0;
  for (i = 0; i < 4; i++) {
    ps[i].L = L;
    ps[i].n = 2000;
    pthread_create(&th[i], NULL, poster, &ps[i]);
  }
  CHECK_STR(run_main(L, STRESS), want);
  for (i = 0; i < 4; i++) pthread_join(th[i], NULL);
  while (nl_pump(L) > 0) ;
  CHECK(posted_runs == 8000);
  lua_close(L);
}

static void test_stress(void)
{
  stress(8);
  stress(1);
}

/* -- Measurements ---------------------------------------------------------------------- */

static void bench(void)
{
  lua_State *L = newstate(1);
  const char *r;
  double t0;
  int i, n = 2000;
  r = run_main(L, "import { square } from 'jobs'\npub async fn run\n  \"#{spawn square(2).await}\"\nend\n");
  (void)r;
  /* One job at a time, awaited: the round trip. */
  lua_settop(L, 0);
  nl_import(L, "jobs");
  lua_getfield(L, -1, "add");
  t0 = now();
  for (i = 0; i < n; i++) {
    lua_pushvalue(L, 2);
    lua_pushnumber(L, i);
    lua_pushnumber(L, 1);
    nl_spawn(L, 2);
    while (nl_taskstate(L, -1) == NL_TASK_PENDING) nl_drain(L);
    lua_settop(L, 2);
  }
  printf("# a small job and back, one at a time (1 pool worker): %.2f us\n", (now() - t0) / n * 1e6);
  /* Many jobs queued at once. */
  t0 = now();
  for (i = 0; i < n; i++) {
    lua_pushvalue(L, 2);
    lua_pushnumber(L, i);
    lua_pushnumber(L, 1);
    nl_spawn(L, 2);
  }
  nl_drain(L);
  printf("# the same, %d queued at once (1 pool worker): %.2f us per job\n", n, (now() - t0) / n * 1e6);
  lua_settop(L, 0);
  lua_close(L);
  L = newstate(0);
  nl_import(L, "jobs");
  lua_getfield(L, -1, "add");
  t0 = now();
  for (i = 0; i < n; i++) {
    lua_pushvalue(L, 2);
    lua_pushnumber(L, i);
    lua_pushnumber(L, 1);
    nl_spawn(L, 2);
    while (nl_taskstate(L, -1) == NL_TASK_PENDING) nl_drain(L);
    lua_settop(L, 2);
  }
  printf("# a small job and back with no worker threads: %.2f us\n", (now() - t0) / n * 1e6);
  lua_close(L);
  printf("# memory of a worker with the runtime and jobs.nl loaded: %.0f KB\n", worker_kb);
}

/* TW_ONLY=name runs only the tests whose name contains it. */
#undef RUN_TEST
#define RUN_TEST(fn) do { \
  if (getenv("TW_ONLY") && !strstr(#fn, getenv("TW_ONLY"))) break; \
  check_test_failed = 0; \
  fn(); \
  printf("%s %d - %s\n", check_test_failed ? "not ok" : "ok", ++test_number, #fn); \
  fflush(stdout); \
} while (0)

int main(int argc, char **argv)
{
  int dobench = argc > 2 && strcmp(argv[2], "--bench") == 0;
  int stressonly = argc > 2 && strcmp(argv[2], "--stress") == 0;
  if (argc > 1 && chdir(argv[1]) != 0) { perror(argv[1]); return 2; }
  NL_VERSION_SYM();
  writefile("jobs.nl", JOBS);
  {
    size_t n = strlen(SVC_IMPORT) + strlen(SVC) + 1;
    char *svc = (char *)malloc(n);
    snprintf(svc, n, "%s%s", SVC_IMPORT, SVC);
    writefile("svc.nl", svc);
    free(svc);
  }
  writefile("exports_stop.nl", EXPORTS_STOP);
  writefile("broken.nl", BROKEN);
  writefile("loads_spawn.nl", LOADS_SPAWN);
  writefile("hostjobs.nl", HOSTJOBS);
  writefile("mod.nl", MOD);
  writefile("modsvc.nl", MOD);
  writefile("pj.nl",
	    "async fn notify\n  Task.sleep(0.05).await\n  Worker.parent.emit \"Hello\", \"from job A's sub-task\"\nend\n"
	    "pub async fn job_a\n  notify().await\n  \"a\"\nend\n"
	    "pub async fn job_b\n  Task.sleep(0.2).await\n  \"b\"\nend\n");
  mkdir("extra", 0755);
  writefile("extra/helper.nl", "pub fn h\n  \"helper found\"\nend\n");
  writefile("wsearch1.nl", "import { h } from 'helper'\npub fn run_h\n  h()\nend\n");
  mkdir("late", 0755);
  writefile("late/latemod.nl", "pub fn l\n  \"late found\"\nend\n");
  mkdir("decls", 0755);
  writefile("decls/game.d.nl", "declare fn host_seven(): Number\n");
  writefile("decls/svc7.nl", "pub fn seven\n  host_seven()\nend\n");
  writefile("decls/jobs7.nl", "pub fn sq(x)\n  x * x\nend\npub fn seven\n  host_seven()\nend\n");
  writefile("pjco.nl",
	    "pub async fn job_a\n"
	    "  let co: any = Coroutine.wrap(fn()\n    Worker.parent.emit \"Hello\", \"from job A's coroutine\"\n  end)\n"
	    "  Task.sleep(0.05).await\n"
	    "  co()\n"
	    "  \"a\"\n"
	    "end\n");
  writefile("pjdone.nl",
	    "pub fn slow\n  let t0 = :os.clock()\n  while :os.clock() - t0 < 0.05\n  end\n  1\nend\n"
	    "pub async fn job_a\n"
	    "  let t = spawn slow()\n"
	    "  t.on_done do |v|\n    Worker.parent.emit \"Hello\", \"from job A's on_done\"\n  end\n"
	    "  Task.sleep(0.15).await\n"
	    "  \"a\"\n"
	    "end\n");
  writefile("selfp.nl",
	    "pub fn inner\n  Worker.parent.emit \"X\", 1\n  \"emitted\"\nend\n"
	    "pub async fn outer\n  (spawn inner()).await\nend\n");
  writefile("pjnone.nl",
	    "let seen = \"not run\"\n"
	    "fn which\n  if Worker.parent == nil\n    \"nil\"\n  else\n    \"a handle\"\n  end\nend\n"
	    "pub async fn job_a\n"
	    "  let t = Task.sleep(0.01)\n"
	    "  t.on_done do |v|\n    seen = which()\n  end\n"
	    "  Task.sleep(0.05).await\n"
	    "  let d = Worker.new 'ptsvc'\n"
	    "  let c = d.point()\n"
	    "  let seen_call = \"not run\"\n"
	    "  c.on_done do |p|\n    seen_call = which()\n  end\n"
	    "  c.await\n"
	    "  d.stop\n"
	    "  \"callback: #{seen}, call's callback: #{seen_call}, job: #{which()}\"\n"
	    "end\n");
  writefile("ptypes.nl",
	    "pub struct P\n  x: Number\nend\n"
	    "fn main\n  print \"the main state's code ran in a worker\"\nend\n"
	    "main() unless Worker.parent\n");
  writefile("ptsvc.nl", "import { P } from 'ptypes'\npub fn point\n  P { x: 1 }\nend\n");
  writefile("pjload.nl",
	    "let outcome = \"pending\"\n"
	    "pub async fn make\n"
	    "  let d = Worker.new 'ptsvc'\n"
	    "  let p = d.point().await\n"
	    "  d.stop\n"
	    "  p\n"
	    "end\n"
	    "pub async fn job_a\n"
	    "  let t = Task.sleep(0.01)\n"
	    "  t.on_done do |v|\n"
	    "    let r = spawn make()\n"
	    "    r.on_done do |p|\n      outcome = \"loaded\"\n    end\n"
	    "    r.on_error do |e|\n      outcome = \"#{typeof(e)}: #{e.message}\"\n    end\n"
	    "    let t0 = :os.clock()\n"
	    "    while :os.clock() - t0 < 0.2\n    end\n"
	    "  end\n"
	    "  while outcome == \"pending\"\n    Task.sleep(0.01).await\n  end\n"
	    "  outcome\n"
	    "end\n");
  writefile("dsvc.nl",
	    "import { job_b } from 'pj'\n"
	    "let heard = []\n"
	    "on Hello do |who|\n  heard[heard.len + 1] = \"dedicated heard #{who}\"\nend\n"
	    "pub async fn start_b\n  Task.sleep(0.01).await\n  (spawn job_b()).await\nend\n"
	    "pub fn heard\n  Table.concat(heard, \",\")\nend\n");
  if (stressonly) {
    RUN_TEST(test_stress);
    return check_summary("test_workers --stress");
  }
  RUN_TEST(test_pool_zero);
  RUN_TEST(test_pool_one);
  RUN_TEST(test_pool_four);
  RUN_TEST(test_count);
  RUN_TEST(test_nl_spawn);
  RUN_TEST(test_init_fails);
  RUN_TEST(test_spawn_while_loading);
  RUN_TEST(test_dedicated_zero);
  RUN_TEST(test_dedicated_threads);
  RUN_TEST(test_maxworkers);
  RUN_TEST(test_workermem);
  RUN_TEST(test_open_under_limit);
  RUN_TEST(test_sandbox_after_spawn);
  RUN_TEST(test_init_over_limit);
  RUN_TEST(test_parent_of_subtasks);
  RUN_TEST(test_discarded_parent);
  RUN_TEST(test_worker_search);
  RUN_TEST(test_parent_in_callbacks);
  RUN_TEST(test_parent_no_job);
  RUN_TEST(test_pool_dies);
  RUN_TEST(test_churn);
  RUN_TEST(test_wakeup);
  RUN_TEST(test_close_busy);
  RUN_TEST(test_stress);
  if (dobench) bench();
  printf("# worker-init calls: %d; unhandled failures reported in the main states: %d\n",
	 init_calls, unhandled_count);
  return check_summary("test_workers");
}
