/*
** A host of nlbundle.h, in C and in C++ (crates/nl_capi/tests/c_abi.rs
** compiles it both ways against libnl_capi.a and runs it): every option
** has an effect it checks, so a header that does not match the library's
** layout fails here.
*/
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "nlbundle.h"

static int failures = 0;
#define CHECK(c) do { if (!(c)) { fprintf(stderr, "%s:%d: CHECK(%s)\n", __FILE__, __LINE__, #c); failures++; } } while (0)

/* The files of the test, in memory. */
static const char *const FILES[][2] = {
  { "addon/main.nl",
    "import { greet } from \"./greet\"\n"
    "if SERVER\n"
    "  :util.AddNetworkString(\"server only\")\n"
    "end\n"
    "print greet(shout(\"client\"))\n" },
  { "addon/greet.nl",
    "pub fn greet(n: String): String\n"
    "  \"hello, #{n}\"\n"
    "end\n" },
  { "addon/shout.d.nl",
    "declare fn shout(s: String): String = :Shout\n" },
};
#define NFILES (sizeof(FILES) / sizeof(FILES[0]))

static int reads = 0, releases = 0;

static const char *find(const char *name) {
  size_t i;
  for (i = 0; i < NFILES; i++)
    if (strcmp(FILES[i][0], name) == 0) return FILES[i][1];
  return NULL;
}

static int t_resolve(void *data, const char *from, const char *path,
		     char *out, size_t size) {
  char name[256];
  (void)data;
  if (from != NULL && strncmp(path, "./", 2) == 0) {
    const char *slash = strrchr(from, '/');
    size_t dir = slash ? (size_t)(slash - from) + 1 : 0;
    snprintf(name, sizeof(name), "%.*s%s", (int)dir, from, path + 2);
  } else {
    snprintf(name, sizeof(name), "%s", path);
  }
  if (find(name) == NULL) strncat(name, ".nl", sizeof(name) - strlen(name) - 1);
  if (find(name) == NULL || strlen(name) + 1 > size) return 0;
  memcpy(out, name, strlen(name) + 1);
  return 1;
}

static const char *t_read(void *data, const char *name, size_t *size) {
  const char *text = find(name);
  (void)data;
  if (text == NULL) return NULL;
  reads++;
  *size = strlen(text);
  return text;
}

static void t_release(void *data, const char *name, const char *buf) {
  (void)data; (void)name; (void)buf;
  releases++;
}

/* A host would load the text on its VM; this one counts. */
static int t_check(void *data, const char *chunkname, const char *text,
		   size_t size, char *err, size_t errsize) {
  (void)chunkname; (void)text; (void)size; (void)err; (void)errsize;
  ++*(int *)data;
  return 1;
}

int main(void) {
  nlb_Loader loader = { t_resolve, t_read, t_release, NULL };
  const char *entries[] = { "addon/main.nl" };
  const char *decls[] = { "addon/shout.d.nl" };
  nlb_Options o;
  nlb_Result *r;
  nlb_SourceMap *map;
  nlb_Text *t;
  const char *file = NULL;
  int line = 0, checked = 0;
  size_t i, n, size = 0;

  CHECK(nlb_abi_version() == NLB_ABI_VERSION);

  nlb_options_init(&o);
  CHECK(o.profile == NLB_PROFILE_PORTABLE && o.checks == 1 && o.case_tables == 1 && o.localize == 1);
  o.profile = NLB_PROFILE_GMOD_CLIENT;
  o.name = "hud";
  o.part_limit = 12000;	/* small: the bundle takes several parts */
  o.sourcemap = 1;
  o.decls = decls;
  o.ndecls = 1;
  o.check = t_check;
  o.check_data = &checked;

  r = nlb_bundle(entries, 1, &o, &loader);
  CHECK(r != NULL);
  for (i = 0; i < nlb_ndiagnostics(r); i++) {
    const nlb_Diagnostic *d = nlb_diagnostic(r, i);
    fprintf(stderr, "%s:%d: %s [%s]\n", d->file, d->line, d->message, d->code);
  }
  CHECK(nlb_ndiagnostics(r) == 0);
  n = nlb_nparts(r);
  CHECK(n >= 3);
  CHECK(strcmp(nlb_part_name(r, 0), "hud.lua") == 0);
  CHECK(strcmp(nlb_part_name(r, 1), "hud_2.lua") == 0);
  for (i = 0; i < n; i++) {
    const char *text = nlb_part(r, i, &size);
    CHECK(text != NULL && strlen(text) == size);
    CHECK(strstr(text, "if SERVER then AddCSLuaFile() return end") != NULL);
    CHECK(strstr(text, "server only") == NULL);
    CHECK(nlb_compressed_size(text, size) <= 12000);
  }
  CHECK(checked == 2);
  CHECK(reads > 0 && reads == releases);
  CHECK(strlen(nlb_build(r)) == 16);

  map = nlb_take_sourcemap(r);
  CHECK(map != NULL);
  CHECK(nlb_take_sourcemap(r) == NULL);
  nlb_free(r);
  CHECK(strcmp(nlb_sourcemap_bundle(map), "hud") == 0);
  CHECK(nlb_sourcemap_lookup(map, "addon/greet.nl", 2, &file, &line) == 1);
  CHECK(file != NULL && strcmp(file, "addon/greet.nl") == 0 && line == 2);
  CHECK(nlb_sourcemap_lookup(map, "nl runtime", 1, &file, &line) == 1);
  CHECK(strcmp(file, "runtime/nl.lua") == 0 && line == 1);
  {
    const nlb_SourceMap *maps[1];
    const char *msg = "addon/greet.nl:2: oops\n\tnl runtime:1: in main chunk";
    maps[0] = map;
    t = nlb_remap(maps, 1, msg, strlen(msg));
    CHECK(strcmp(nlb_text(t, NULL), "addon/greet.nl:2: oops\n\truntime/nl.lua:1: in main chunk") == 0);
    nlb_text_free(t);
  }
  t = nlb_sourcemap_json(map);
  {
    char err[128];
    const char *json = nlb_text(t, &size);
    nlb_SourceMap *again = nlb_sourcemap_parse(json, size, err, sizeof(err));
    CHECK(again != NULL);
    CHECK(strcmp(nlb_sourcemap_build(again), nlb_sourcemap_build(map)) == 0);
    nlb_sourcemap_free(again);
    CHECK(nlb_sourcemap_parse("{", 1, err, sizeof(err)) == NULL && err[0] != '\0');
  }
  nlb_text_free(t);
  nlb_sourcemap_free(map);

  /* A bundle without the runtime, which includes the runtime's file; the
  ** runtime alone, its first part including the others.
  */
  nlb_options_init(&o);
  CHECK(o.runtime_include == NULL && o.include_parts == NULL);
  o.profile = NLB_PROFILE_GMOD_CLIENT;
  o.name = "hud";
  o.decls = decls;
  o.ndecls = 1;
  o.runtime_include = "nl/runtime.lua";
  r = nlb_bundle(entries, 1, &o, &loader);
  CHECK(r != NULL && nlb_ndiagnostics(r) == 0 && nlb_nparts(r) == 1);
  {
    const char *text = nlb_part(r, 0, &size);
    CHECK(strstr(text, "include(\"nl/runtime.lua\")") != NULL);
    CHECK(strstr(text, "nl runtime") == NULL);
  }
  nlb_free(r);
  nlb_options_init(&o);
  o.profile = NLB_PROFILE_GMOD_CLIENT;
  o.name = "runtime";
  o.part_limit = 12000;
  o.include_parts = "nl/runtime.lua";
  r = nlb_runtime(&o);
  CHECK(r != NULL && nlb_ndiagnostics(r) == 0 && nlb_nparts(r) >= 3);
  CHECK(strcmp(nlb_part_name(r, 1), "runtime_2.lua") == 0);
  CHECK(strstr(nlb_part(r, 0, NULL), "include(\"nl/runtime_2.lua\")") != NULL);
  CHECK(strstr(nlb_part(r, 1, NULL), "include(") == NULL);
  nlb_free(r);

  {
    const char *glua = "local a = 1 // one\nif a != 2 then /* two */ print(a) end\n";
    t = nlb_minify(glua, strlen(glua), NLB_MINIFY_GLUA);
    CHECK(nlb_text_error(t) == NULL);
    CHECK(strcmp(nlb_text(t, &size), "local a=1\nif a!=2 then print(a)end\n") == 0);
    nlb_text_free(t);
  }

  if (failures == 0) printf("nlbundle_test: ok\n");
  return failures == 0 ? 0 : 1;
}
