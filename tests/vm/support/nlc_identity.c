/*
** Test double for libnlc: an "identity" nl_compile (nlc.h) that treats its
** input as Lua source and wraps it in the chunk protocol of
** crates/nl_capi/src/driver.rs:
**
**   lua_load(chunk) gives P; P(rt) returns the main function.
**
** The wrapper is put on the first line so that line numbers stay those of the
** source:  local __nl_rt = ...; return function(...) <source>\nend
** Inside the source, `__nl_rt` is the runtime table.
**
** Directives, one per line, let tests drive the parts of the boundary that
** real NL code would reach:
**   --@error CODE TEXT      an error diagnostic at this line
**   --@warning CODE TEXT    a warning
**   --@lint CODE TEXT       a lint
**   --@options              a lint whose message is "checks=C strip=S
**                           localize=L"
**   --@import PATH          resolve PATH through the loader (from = this
**                           file's name for "@name" chunks, else NULL) and
**                           read it; an error diagnostic when either fails
** A compile with an error diagnostic returns no chunk.
*/

#include <stdlib.h>
#include <string.h>
#include <stdio.h>

#include "nlc.h"

typedef struct Diag {
  nlc_Diagnostic d;
  char *code, *message, *file;
} Diag;

struct nlc_Result {
  char *chunk;
  size_t chunklen;
  Diag *diags;
  size_t ndiags, cap;
};

static char *dupn(const char *s, size_t n)
{
  char *p = (char *)malloc(n + 1);
  if (p) { memcpy(p, s, n); p[n] = '\0'; }
  return p;
}

static void add_diag(nlc_Result *r, int sev, const char *code, size_t codelen,
		     const char *msg, size_t msglen, const char *file, int line)
{
  Diag *d;
  if (r->ndiags == r->cap) {
    size_t cap = r->cap ? r->cap * 2 : 4;
    Diag *nd = (Diag *)realloc(r->diags, cap * sizeof(Diag));
    if (!nd) return;
    r->diags = nd;
    r->cap = cap;
  }
  d = &r->diags[r->ndiags++];
  d->code = dupn(code, codelen);
  d->message = dupn(msg, msglen);
  d->file = dupn(file, strlen(file));
  d->d.severity = sev;
  d->d.code = d->code;
  d->d.message = d->message;
  d->d.file = d->file;
  d->d.line = line;
  d->d.column = 1;
  d->d.endline = line;
  d->d.endcolumn = 1;
}

static const char WRAP_HEAD[] = "local __nl_rt = ...; return function(...) ";
static const char WRAP_TAIL[] = "\nend";

nlc_Result *nl_compile(const char *source, size_t size, const char *chunkname,
		       const nlc_Options *options, const nlc_Loader *loader)
{
  nlc_Result *r = (nlc_Result *)calloc(1, sizeof(nlc_Result));
  const char *file = (chunkname[0] == '@' || chunkname[0] == '=') ?
		     chunkname + 1 : chunkname;
  const char *from = chunkname[0] == '@' ? chunkname + 1 : NULL;
  const char *p = source, *end = source + size;
  int line = 1, errors = 0;
  if (!r) return NULL;
  while (p < end) {
    const char *eol = memchr(p, '\n', (size_t)(end - p));
    const char *le = eol ? eol : end;
    if (le - p >= 3 && memcmp(p, "--@", 3) == 0) {
      const char *w = p + 3, *we = w;
      int sev = -1;
      while (we < le && *we != ' ') we++;
      if (we - w == 5 && memcmp(w, "error", 5) == 0) sev = NLC_DIAG_ERROR;
      else if (we - w == 7 && memcmp(w, "warning", 7) == 0) sev = NLC_DIAG_WARNING;
      else if (we - w == 4 && memcmp(w, "lint", 4) == 0) sev = NLC_DIAG_LINT;
      if (sev >= 0) {
	const char *c = we < le ? we + 1 : le, *ce = c;
	while (ce < le && *ce != ' ') ce++;
	add_diag(r, sev, c, (size_t)(ce - c), ce < le ? ce + 1 : le,
		 (size_t)(le - (ce < le ? ce + 1 : le)), file, line);
	if (sev == NLC_DIAG_ERROR) errors++;
      } else if (we - w == 7 && memcmp(w, "options", 7) == 0) {
	char msg[64];
	snprintf(msg, sizeof(msg), "checks=%d strip=%d localize=%d",
		 options ? options->checks : -1, options ? options->strip : -1,
		 options ? options->localize : -1);
	add_diag(r, NLC_DIAG_LINT, "options", 7, msg, strlen(msg), file, line);
      } else if (we - w == 6 && memcmp(w, "import", 6) == 0) {
	char path[1024], out[4096];
	size_t plen = (size_t)(le - (we < le ? we + 1 : le));
	if (plen >= sizeof(path)) plen = sizeof(path) - 1;
	memcpy(path, we < le ? we + 1 : le, plen);
	path[plen] = '\0';
	if (!loader->resolve(loader->data, from, path, out, sizeof(out))) {
	  char msg[1200];
	  snprintf(msg, sizeof(msg), "cannot find import '%s'", path);
	  add_diag(r, NLC_DIAG_ERROR, "unresolved-import", 17, msg,
		   strlen(msg), file, line);
	  errors++;
	} else {
	  size_t sz = 0;
	  const char *buf = loader->read(loader->data, out, &sz);
	  if (!buf) {
	    char msg[4200];
	    snprintf(msg, sizeof(msg), "cannot read '%s'", out);
	    add_diag(r, NLC_DIAG_ERROR, "unreadable-import", 17, msg,
		     strlen(msg), file, line);
	    errors++;
	  } else if (loader->release) {
	    loader->release(loader->data, out, buf);
	  }
	}
      }
    }
    if (!eol) break;
    p = eol + 1;
    line++;
  }
  /* Errors first (driver.rs: errors first within a file). */
  if (errors && r->ndiags > 1) {
    size_t i, j = 0;
    Diag *sorted = (Diag *)malloc(r->ndiags * sizeof(Diag));
    if (sorted) {
      for (i = 0; i < r->ndiags; i++)
	if (r->diags[i].d.severity == NLC_DIAG_ERROR) sorted[j++] = r->diags[i];
      for (i = 0; i < r->ndiags; i++)
	if (r->diags[i].d.severity != NLC_DIAG_ERROR) sorted[j++] = r->diags[i];
      free(r->diags);
      r->diags = sorted;
      r->cap = r->ndiags;
    }
  }
  if (!errors) {
    size_t hl = sizeof(WRAP_HEAD) - 1, tl = sizeof(WRAP_TAIL) - 1;
    r->chunk = (char *)malloc(hl + size + tl);
    if (!r->chunk) { nlc_free(r); return NULL; }
    memcpy(r->chunk, WRAP_HEAD, hl);
    memcpy(r->chunk + hl, source, size);
    memcpy(r->chunk + hl + size, WRAP_TAIL, tl);
    r->chunklen = hl + size + tl;
  }
  return r;
}

const char *nlc_chunk(const nlc_Result *r, size_t *size)
{
  if (size) *size = r->chunk ? r->chunklen : 0;
  return r->chunk;
}

size_t nlc_ndiagnostics(const nlc_Result *r)
{
  return r->ndiags;
}

const nlc_Diagnostic *nlc_diagnostic(const nlc_Result *r, size_t i)
{
  return i < r->ndiags ? &r->diags[i].d : NULL;
}

void nlc_free(nlc_Result *r)
{
  size_t i;
  if (!r) return;
  for (i = 0; i < r->ndiags; i++) {
    free(r->diags[i].code);
    free(r->diags[i].message);
    free(r->diags[i].file);
  }
  free(r->diags);
  free(r->chunk);
  free(r);
}
