/* glua_dump FILE OUT: loads FILE as GLua (NL_OPT_GLUA) under the chunk name
** "=chunk" and writes its bytecode, debug information included (lines,
** names), to OUT; exit status 1 with "ERR FILE: message" when it does not
** load. The VM's PRNG gets a fixed seed, and on Linux the process runs
** without address randomization (it execs itself once with
** ADDR_NO_RANDOMIZE), so that string hashes, and with them the order of
** the keys of a constant table in a dump, are the same in every run: two
** files dump the same when they are the same program on the same lines.
** Built and run by tests/vm/minify_corpus.sh.
*/
#include <errno.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#ifdef __linux__
#include <sys/personality.h>
#include <unistd.h>
#endif
#include "lua.h"
#include "lauxlib.h"
#include "nl.h"

/* lj_prng.c seeds through syscall(SYS_getrandom, ...) on Linux. */
long syscall(long n, ...)
{
  va_list ap;
  void *buf;
  size_t len;
  if (n != SYS_getrandom) { errno = ENOSYS; return -1; }
  va_start(ap, n);
  buf = va_arg(ap, void *);
  len = va_arg(ap, size_t);
  va_end(ap);
  memset(buf, 0x5a, len);
  return (long)len;
}

static int writer(lua_State *L, const void *p, size_t sz, void *ud)
{
  (void)L;
  return fwrite(p, 1, sz, (FILE *)ud) != sz;
}

int main(int argc, char **argv)
{
  lua_State *L;
  FILE *f, *o;
  char *buf;
  long n;
  if (argc != 3) { fprintf(stderr, "usage: glua_dump FILE OUT\n"); return 2; }
#ifdef __linux__
  {
    int pers = personality(0xffffffff);
    if (pers != -1 && !(pers & ADDR_NO_RANDOMIZE) &&
	personality((unsigned long)pers | ADDR_NO_RANDOMIZE) != -1)
      execv("/proc/self/exe", argv);  /* On failure, go on as we are. */
  }
#endif
  f = fopen(argv[1], "rb");
  if (f == NULL) { fprintf(stderr, "cannot read %s\n", argv[1]); return 2; }
  fseek(f, 0, SEEK_END);
  n = ftell(f);
  fseek(f, 0, SEEK_SET);
  buf = malloc((size_t)n + 1);
  if (buf == NULL || fread(buf, 1, (size_t)n, f) != (size_t)n) return 2;
  fclose(f);
  L = luaL_newstate();
  nl_setoption(L, NL_OPT_GLUA, 1);
  if (luaL_loadbuffer(L, buf, (size_t)n, "=chunk") != 0) {
    printf("ERR %s: %s\n", argv[1], lua_tostring(L, -1));
    return 1;
  }
  o = fopen(argv[2], "wb");
  if (o == NULL || lua_dump(L, writer, o) != 0) return 2;
  fclose(o);
  lua_close(L);
  free(buf);
  return 0;
}
