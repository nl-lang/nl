/*
** The thread rule with real threads:
** nl_post from many threads, nl_pump on the owner, the wakeup callback, tasks
** finished from another thread, and lua_close with callbacks still queued.
** tests/vm/run.sh also runs it built with ThreadSanitizer.
*/

#include <pthread.h>
#include <sched.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <unistd.h>

#include "lua.h"
#include "lauxlib.h"
#include "lualib.h"
#include "nl.h"

#include "support/check.h"

#define NTHREADS 8
#define NPOSTS 5000

typedef struct Msg {
  int thread, seq;
} Msg;

typedef struct Poster {
  lua_State *L;
  int id;
  Msg *msgs;
  int nposts;
  int yield_every;
} Poster;

static int last_seq[NTHREADS];
static int order_errors, delivered, null_calls;

/* Runs on the owner thread: touches the state, checks per-thread order. */
static void on_msg(lua_State *L, void *data)
{
  Msg *m = (Msg *)data;
  if (!L) { null_calls++; return; }
  if (m->seq != last_seq[m->thread] + 1) order_errors++;
  last_seq[m->thread] = m->seq;
  lua_getglobal(L, "count");
  lua_pushinteger(L, m->thread);
  lua_call(L, 1, 0);
  delivered++;
}

static void *poster(void *arg)
{
  Poster *p = (Poster *)arg;
  int i;
  for (i = 0; i < p->nposts; i++) {
    p->msgs[i].thread = p->id;
    p->msgs[i].seq = i + 1;
    while (!nl_post(p->L, on_msg, &p->msgs[i])) {}
    if (p->yield_every && i % p->yield_every == 0) sched_yield();
  }
  return NULL;
}

static void test_many_posters(void)
{
  lua_State *L = luaL_newstate();
  pthread_t th[NTHREADS];
  Poster ps[NTHREADS];
  int i, total = NTHREADS * NPOSTS, pumps = 0;
  luaL_openlibs(L);
  luaL_dostring(L, "counts = {} function count(t) counts[t] = (counts[t] or 0) + 1 end");
  memset(last_seq, 0, sizeof(last_seq));
  order_errors = delivered = 0;
  for (i = 0; i < NTHREADS; i++) {
    ps[i].L = L;
    ps[i].id = i;
    ps[i].nposts = NPOSTS;
    ps[i].yield_every = 64;
    ps[i].msgs = (Msg *)calloc(NPOSTS, sizeof(Msg));
    pthread_create(&th[i], NULL, poster, &ps[i]);
  }
  while (delivered < total) {
    nl_pump(L);
    pumps++;
  }
  for (i = 0; i < NTHREADS; i++) pthread_join(th[i], NULL);
  CHECK(nl_pump(L) == 0);
  CHECK(delivered == total);
  CHECK(order_errors == 0);
  luaL_dostring(L, "local s = 0 for _, n in pairs(counts) do s = s + n end return s");
  CHECK(lua_tointeger(L, -1) == total);
  lua_pop(L, 1);
  printf("# %d messages from %d threads in %d pumps\n", total, NTHREADS, pumps);
  lua_close(L);
  for (i = 0; i < NTHREADS; i++) free(ps[i].msgs);
}

/* An event-loop host: it sleeps until the wakeup fires, then pumps. */
typedef struct Loop {
  pthread_mutex_t mtx;
  pthread_cond_t cond;
  int signalled, wakeups;
} Loop;

static void loop_wakeup(void *data)
{
  Loop *lp = (Loop *)data;
  pthread_mutex_lock(&lp->mtx);
  lp->signalled = 1;
  lp->wakeups++;
  pthread_cond_signal(&lp->cond);
  pthread_mutex_unlock(&lp->mtx);
}

static void test_wakeup_driven_loop(void)
{
  lua_State *L = luaL_newstate();
  Loop lp;
  pthread_t th[4];
  Poster ps[4];
  int i, total = 4 * 500, wakeups;
  luaL_openlibs(L);
  luaL_dostring(L, "function count() end");
  memset(last_seq, 0, sizeof(last_seq));
  order_errors = delivered = 0;
  pthread_mutex_init(&lp.mtx, NULL);
  pthread_cond_init(&lp.cond, NULL);
  lp.signalled = lp.wakeups = 0;
  nl_setwakeup(L, loop_wakeup, &lp);
  for (i = 0; i < 4; i++) {
    ps[i].L = L;
    ps[i].id = i;
    ps[i].nposts = 500;
    ps[i].yield_every = 1;
    ps[i].msgs = (Msg *)calloc(500, sizeof(Msg));
    pthread_create(&th[i], NULL, poster, &ps[i]);
  }
  while (delivered < total) {
    pthread_mutex_lock(&lp.mtx);
    while (!lp.signalled) pthread_cond_wait(&lp.cond, &lp.mtx);
    lp.signalled = 0;
    pthread_mutex_unlock(&lp.mtx);
    nl_pump(L);
  }
  for (i = 0; i < 4; i++) pthread_join(th[i], NULL);
  nl_pump(L);
  pthread_mutex_lock(&lp.mtx);
  wakeups = lp.wakeups;
  pthread_mutex_unlock(&lp.mtx);
  CHECK(delivered == total && order_errors == 0);
  CHECK(wakeups >= 1 && wakeups <= total);
  printf("# %d messages, %d wakeups\n", total, wakeups);
  nl_setwakeup(L, NULL, NULL);
  lua_close(L);
  for (i = 0; i < 4; i++) free(ps[i].msgs);
  pthread_cond_destroy(&lp.cond);
  pthread_mutex_destroy(&lp.mtx);
}

/* A host function that starts work on another thread and returns a task,
** finished at the next pump.
*/
typedef struct Request {
  lua_State *L;
  int task;
  int input;
  int result;
  pthread_t th;
} Request;

static int finished_requests, freed_requests;

static void finish_request(lua_State *L, void *data)
{
  Request *r = (Request *)data;
  if (L) {
    lua_rawgeti(L, LUA_REGISTRYINDEX, r->task);
    lua_pushinteger(L, r->result);
    nl_resolve(L, -2);
    lua_pop(L, 1);
    luaL_unref(L, LUA_REGISTRYINDEX, r->task);
    finished_requests++;
  }
  freed_requests++;
}

static void *request_thread(void *arg)
{
  Request *r = (Request *)arg;
  usleep(1000 * (r->input % 5));
  r->result = r->input * r->input;
  nl_post(r->L, finish_request, r);
  return NULL;
}

static Request requests[32];
static int nrequests;

static int square_async(lua_State *L)
{
  Request *r = &requests[nrequests++];
  r->L = L;
  r->input = (int)luaL_checkinteger(L, 1);
  nl_newtask(L);
  lua_pushvalue(L, -1);
  r->task = luaL_ref(L, LUA_REGISTRYINDEX);
  pthread_create(&r->th, NULL, request_thread, r);
  return 1;
}

static void test_tasks_finished_from_threads(void)
{
  lua_State *L = luaL_newstate();
  int i;
  luaL_openlibs(L);
  nrequests = finished_requests = freed_requests = 0;
  lua_register(L, "square_async", square_async);
  luaL_dostring(L, "results = {} for i = 1, 20 do "
		   "  square_async(i):on_done(function(v) results[i] = v end) end");
  while (finished_requests < 20) nl_pump(L);
  for (i = 0; i < nrequests; i++) pthread_join(requests[i].th, NULL);
  luaL_dostring(L, "for i = 1, 20 do assert(results[i] == i * i) end return true");
  CHECK(lua_toboolean(L, -1));
  lua_pop(L, 1);
  lua_close(L);
  CHECK(freed_requests == 20);
}

/* Callbacks still queued at lua_close are called once with L == NULL. */
static void test_close_with_queued_callbacks(void)
{
  lua_State *L = luaL_newstate();
  pthread_t th[4];
  Poster ps[4];
  int i, pumped;
  luaL_openlibs(L);
  luaL_dostring(L, "function count() end");
  memset(last_seq, 0, sizeof(last_seq));
  order_errors = delivered = null_calls = 0;
  for (i = 0; i < 4; i++) {
    ps[i].L = L;
    ps[i].id = i;
    ps[i].nposts = 1000;
    ps[i].yield_every = 0;
    ps[i].msgs = (Msg *)calloc(1000, sizeof(Msg));
    pthread_create(&th[i], NULL, poster, &ps[i]);
  }
  nl_pump(L);  /* Some, while the threads post. */
  /* The host stops its threads from posting before it calls lua_close. */
  for (i = 0; i < 4; i++) pthread_join(th[i], NULL);
  pumped = delivered;
  lua_close(L);
  CHECK(pumped + null_calls == 4000);
  CHECK(order_errors == 0);
  printf("# %d delivered, %d freed at close\n", pumped, null_calls);
  for (i = 0; i < 4; i++) free(ps[i].msgs);
}

/* Several states, each with its own inbox and posters, in parallel. */
typedef struct Owner {
  int id, sum;
  pthread_t th;
} Owner;

static void add_one(lua_State *L, void *data)
{
  int *sum = (int *)data;
  if (L) (*sum)++;
}

typedef struct SimplePoster {
  lua_State *L;
  int *sum;
} SimplePoster;

static void *simple_poster(void *arg)
{
  SimplePoster *sp = (SimplePoster *)arg;
  int i;
  for (i = 0; i < 2000; i++) nl_post(sp->L, add_one, sp->sum);
  return NULL;
}

static void *owner_thread(void *arg)
{
  Owner *o = (Owner *)arg;
  lua_State *L = luaL_newstate();
  pthread_t p[2];
  SimplePoster sp;
  luaL_openlibs(L);
  sp.L = L;
  sp.sum = &o->sum;
  pthread_create(&p[0], NULL, simple_poster, &sp);
  pthread_create(&p[1], NULL, simple_poster, &sp);
  while (o->sum < 4000) nl_pump(L);
  pthread_join(p[0], NULL);
  pthread_join(p[1], NULL);
  lua_close(L);
  return NULL;
}

static void test_independent_states(void)
{
  Owner o[4];
  int i;
  for (i = 0; i < 4; i++) {
    o[i].id = i;
    o[i].sum = 0;
    pthread_create(&o[i].th, NULL, owner_thread, &o[i]);
  }
  for (i = 0; i < 4; i++) {
    pthread_join(o[i].th, NULL);
    CHECK(o[i].sum == 4000);
  }
}


/* Once nl_setwakeup returns, the old wakeup is never called again,
** so the host may free its data at once. Two threads post while the owner
** pumps and replaces the wakeup each time, freeing the old data (after
** marking it dead, which the wakeup checks; ASan and TSan see the rest).
*/
#define WAKE_LIVE 0x57414b45
typedef struct WakeData {
  int magic;
  long hits;
} WakeData;

static volatile int race_stop;
static volatile long race_dead_calls;
static long race_posted, race_delivered;
static pthread_mutex_t race_mtx = PTHREAD_MUTEX_INITIALIZER;

static void race_wake(void *data)
{
  /* Wakeups may run on two posting threads at once: the
  ** counters are under the test's own lock.
  */
  WakeData *w = (WakeData *)data;
  pthread_mutex_lock(&race_mtx);
  if (w->magic != WAKE_LIVE) race_dead_calls++;
  w->hits++;
  pthread_mutex_unlock(&race_mtx);
}

static void race_cb(lua_State *L, void *data)
{
  (void)data;
  if (L) {
    pthread_mutex_lock(&race_mtx);
    race_delivered++;
    pthread_mutex_unlock(&race_mtx);
  }
}

static void *race_poster(void *arg)
{
  lua_State *L = (lua_State *)arg;
  for (;;) {
    long inflight;
    pthread_mutex_lock(&race_mtx);
    if (race_stop) { pthread_mutex_unlock(&race_mtx); break; }
    inflight = race_posted - race_delivered;
    if (inflight <= 2) race_posted++;
    pthread_mutex_unlock(&race_mtx);
    if (inflight <= 2) nl_post(L, race_cb, NULL);
    else sched_yield();
  }
  return NULL;
}

static void test_setwakeup_while_posting(void)
{
  lua_State *L = luaL_newstate();
  pthread_t th[2];
  WakeData *cur;
  long i;
  luaL_openlibs(L);
  race_stop = 0; race_dead_calls = 0; race_posted = race_delivered = 0;
  cur = (WakeData *)calloc(1, sizeof(WakeData));
  cur->magic = WAKE_LIVE;
  nl_setwakeup(L, race_wake, cur);
  for (i = 0; i < 2; i++) pthread_create(&th[i], NULL, race_poster, L);
  for (i = 0; i < 20000; i++) {
    WakeData *nw = (WakeData *)calloc(1, sizeof(WakeData));
    nw->magic = WAKE_LIVE;
    nl_pump(L);
    nl_setwakeup(L, race_wake, nw);
    cur->magic = 0;  /* The old wakeup is no longer installed. */
    free(cur);
    cur = nw;
  }
  pthread_mutex_lock(&race_mtx);
  race_stop = 1;
  pthread_mutex_unlock(&race_mtx);
  for (i = 0; i < 2; i++) pthread_join(th[i], NULL);
  nl_setwakeup(L, NULL, NULL);
  cur->magic = 0;
  free(cur);
  while (nl_pump(L) > 0) {}
  CHECK(race_dead_calls == 0);
  printf("# %ld posted while the wakeup was replaced 20000 times\n", race_posted);
  lua_close(L);
}

int main(void)
{
  RUN_TEST(test_many_posters);
  RUN_TEST(test_wakeup_driven_loop);
  RUN_TEST(test_tasks_finished_from_threads);
  RUN_TEST(test_close_with_queued_callbacks);
  RUN_TEST(test_independent_states);
  RUN_TEST(test_setwakeup_while_posting);
  return check_summary("test_threads");
}
