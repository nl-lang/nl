/*
** nlbundle.h: NL's packager, minifier and sourcemaps, for a host in C or
** C++ (a Garry's Mod lua_shared built on NL's VM, a build tool).
**
** libnlc implements it (crates/nl_capi in the NL repository:
** src/capi_bundle.rs over src/bundle.rs, src/minify.rs, src/sourcemap.rs):
** the static library the VM links, `cargo build --release -p nl_capi`.
** This header stands alone: it needs neither lua.h nor nl.h, and its
** loader and diagnostic types have the layout of nl.h's, so a host passes
** its nl_Loader as an nlb_Loader.
**
** Everything here is pure: nothing takes a lua_State, runs Lua or touches
** the file system. Files are read through the loader; whether a chunk
** loads on LuaJIT is asked of the host (nlb_Options.check), which has a
** VM. Calls may come from any thread; the callbacks of one call run on the
** thread that made it. No Rust panic crosses this boundary: it becomes a
** NULL result or a diagnostic with the code "internal-error". Memory the
** library returns is freed only by its own nlb_*_free functions; a string
** it returns stays valid until the object it came from is freed.
**
** `nlc bundle`, `nlc remap` and `nlc build --minify` are the command line
** of the same code.
*/

#ifndef _NLBUNDLE_H
#define _NLBUNDLE_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The version of this header's types and functions; compare with
** nlb_abi_version() at run time when the library is linked separately.
*/
#define NLB_ABI_VERSION		2
int nlb_abi_version(void);

/* Same layout and contract as nl_Loader (nl.h). */
typedef struct nlb_Loader {
  /* Copies the canonical name of the file that `path` (written in an import
  ** of `from`, or of no file when `from` is NULL) means into out[size].
  ** Returns 1, or 0 when there is no such file.
  */
  int (*resolve)(void *data, const char *from, const char *path,
		 char *out, size_t size);
  /* The contents of a canonical name, or NULL. */
  const char *(*read)(void *data, const char *name, size_t *size);
  /* Called once for each successful read, when the library is done. */
  void (*release)(void *data, const char *name, const char *buf);
  void *data;
} nlb_Loader;

#define NLB_DIAG_ERROR		0
#define NLB_DIAG_WARNING	1
#define NLB_DIAG_LINT		2

/* Same layout as nl_Diagnostic (nl.h). */
typedef struct nlb_Diagnostic {
  int severity;			/* NLB_DIAG_* */
  const char *code;		/* Stable identifier: "wrong-realm", "not-in-gmod", ... */
  const char *message;
  const char *file;		/* Canonical name of the file. */
  int line, column;		/* 1-based; the column counts characters. */
  int endline, endcolumn;
} nlb_Diagnostic;

/* ---- The packager --------------------------------------------------- */

/* A bundle is NL files compiled into Lua for a stock LuaJIT, 2.0 or 2.1,
** with NL's runtime: one Lua file, or with NLB_PROFILE_GMOD_CLIENT, as
** many as Garry's Mod's limit on a file it sends to clients takes
** (64 KiB, LZMA-compressed). Run the file(s) on the client, in any order:
** the last part to arrive runs the program. Each part starts with
** `if SERVER then AddCSLuaFile() return end`.
**
** The gmod-client profile rejects, at compile time, what the stock client
** lacks: what gmod removes from Lua (io, loadstring, most of os, ...),
** LuaJIT 2.1-only functions, gmod functions, methods and values of the
** server or menu realm. Code under `if SERVER` (and other conditions of
** SERVER and CLIENT that are known on the client) is neither checked nor
** compiled. Garry's Mod's API declarations are built in. The chunks are
** minified (line breaks stay), and must load (nlb_Options.check).
*/

#define NLB_PROFILE_PORTABLE	0	/* Stock LuaJIT, anywhere. */
#define NLB_PROFILE_GMOD_CLIENT	1	/* Garry's Mod's client realm. */

/* Whether a chunk loads on LuaJIT: 1 when it does; 0 when it does not,
** with the error message ("name:line: message", as lua_load gives it)
** written NUL-terminated to err[errsize]. `chunkname` is "@name". A host
** with NL's VM loads the text with luaL_loadbufferx, in GLua mode
** (nl_setoption(L, NL_OPT_GLUA, 1)) for gmod, and pops the function.
*/
typedef int (*nlb_Check)(void *data, const char *chunkname,
			 const char *text, size_t size,
			 char *err, size_t errsize);

typedef struct nlb_Options {
  int profile;			/* NLB_PROFILE_*; default PORTABLE */
  /* The bundle's name: its files are "<name>.lua", "<name>_2.lua", ...;
  ** the key under which its parts meet; the sourcemap's "bundle". No `/`,
  ** `\`, backquote or control character. NULL: "bundle". */
  const char *name;
  int checks;			/* Runtime type checks (NL_OPT_CHECKS); default 1 */
  int minify;			/* Minify a portable bundle; gmod always is. */
  int case_tables;		/* The String library's Unicode case tables; default 1 */
  /* gmod: the most bytes a file may take, LZMA-compressed; 0 for the
  ** default (60000: gmod takes 65536, and its encoder is not this one). */
  size_t part_limit;
  /* Make the sourcemap (nlb_take_sourcemap). The chunks then carry no
  ** line maps of their own: the map is the one translation of positions. */
  int sourcemap;
  int localize;			/* NL_OPT_LOCALIZE: 0, 1 (default), 2 */
  /* Declaration files in scope (`declare ...`): names read through the
  ** loader. gmod's are built in for the gmod profile. */
  const char *const *decls;
  size_t ndecls;
  nlb_Check check;		/* NULL: chunks are taken on trust. */
  void *check_data;
  /* Leave NL's runtime out of the bundle: it runs gmod's include of this
  ** Lua path (as include takes it: "nl/runtime.lua") when the Lua state has
  ** no runtime yet. nlb_runtime makes the file to put there; every bundle
  ** then shares it, and clients download it once. NULL: the bundle
  ** carries the runtime. */
  const char *runtime_include;
  /* gmod: the Lua path of the bundle's first file ("autorun/client/x.lua").
  ** A bundle in parts then has its first part include the others
  ** ("autorun/client/x_2.lua", ...) before it keeps its own piece, so that
  ** running or including the first file alone runs the bundle. NULL: the
  ** parts run by themselves (lua/autorun/client/), in any order. */
  const char *include_parts;
} nlb_Options;

typedef struct nlb_Result nlb_Result;

/* Fills `o` with the defaults. */
void nlb_options_init(nlb_Options *o);

/* Bundles the entry files and everything they import (and include by a
** literal path). Each entry is resolved as loader->resolve resolves a path
** of no file (from = NULL). `options` NULL: the defaults. Returns NULL
** only when the result cannot be built at all.
*/
nlb_Result *nlb_bundle(const char *const *entries, size_t nentries,
		       const nlb_Options *options, const nlb_Loader *loader);

/* The runtime alone, the file that bundles made with runtime_include
** include: it installs NL's runtime (with its gmod host and, unless
** case_tables is 0, the String library's case tables) in a Lua state that
** has none. Of the options, profile, name, minify, case_tables,
** part_limit, include_parts and sourcemap apply; its files are named and
** split as a bundle's. Read it as an nlb_bundle result.
*/
nlb_Result *nlb_runtime(const nlb_Options *options);

/* The files of the bundle, in order: none when a diagnostic is an error.
** nlb_part's text is NUL-terminated; *size (when size is not NULL) does
** not count the NUL.
*/
size_t nlb_nparts(const nlb_Result *r);
const char *nlb_part(const nlb_Result *r, size_t i, size_t *size);
const char *nlb_part_name(const nlb_Result *r, size_t i);
/* A hash of the bundle's content ("" when none was made). */
const char *nlb_build(const nlb_Result *r);

/* Every diagnostic of every file, then the bundle's own. */
size_t nlb_ndiagnostics(const nlb_Result *r);
const nlb_Diagnostic *nlb_diagnostic(const nlb_Result *r, size_t i);

typedef struct nlb_SourceMap nlb_SourceMap;

/* The bundle's sourcemap (nlb_Options.sourcemap), now the caller's: free
** it with nlb_sourcemap_free. NULL without one, or once taken.
*/
nlb_SourceMap *nlb_take_sourcemap(nlb_Result *r);

void nlb_free(nlb_Result *r);

/* ---- Text results ------------------------------------------------------ */

typedef struct nlb_Text nlb_Text;

/* The text (NUL-terminated; *size without the NUL), or NULL when the
** operation failed; then nlb_text_error says why.
*/
const char *nlb_text(const nlb_Text *t, size_t *size);
const char *nlb_text_error(const nlb_Text *t);
void nlb_text_free(nlb_Text *t);

/* ---- The minifier ------------------------------------------------------ */

/* Comments, indentation and the spaces that separate nothing go; every
** line break stays, so errors and traces show the same lines.
** NLB_MINIFY_GLUA reads GLua: `//` line comments and slash-star block
** comments, `!=`, `&&`, `||`. NLB_MINIFY_ESCAPE_MACRO spells DEFINE_BASECLASS inside quoted
** strings with an escape, so that gmod's textual replacement of it leaves
** the string as written: for compiled code, not for a GLua script, which
** expects the replacement. Fails (nlb_text_error) on an unfinished string
** or long comment. Returns NULL only when the result cannot be built.
*/
#define NLB_MINIFY_GLUA			1
#define NLB_MINIFY_ESCAPE_MACRO		2
nlb_Text *nlb_minify(const char *src, size_t size, int flags);

/* The size of data compressed as gmod compresses the files it sends to
** clients (LZMA); gmod sends a file of at most 64 KiB.
*/
size_t nlb_compressed_size(const char *data, size_t size);

/* ---- Sourcemaps --------------------------------------------------------- */

/* A sourcemap maps the positions LuaJIT reports, a chunk's `short_src` and
** a line (lua_Debug.short_src and .currentline; the "name:line:" of an
** error message), to the NL or runtime file and line they come from, for
** every chunk of a bundle: the NL files under their canonical names, and
** "nl runtime" and "nl host". A name LuaJIT shortened to "...end/of/name"
** is matched by its end. Maps are immutable: lookups from many threads
** are fine.
*/

/* A sourcemap from its JSON (nlb_sourcemap_json, `nlc bundle --map`), or
** NULL with the reason in err[errsize].
*/
nlb_SourceMap *nlb_sourcemap_parse(const char *json, size_t size,
				   char *err, size_t errsize);
/* The map as JSON, to keep it. */
nlb_Text *nlb_sourcemap_json(const nlb_SourceMap *m);
const char *nlb_sourcemap_bundle(const nlb_SourceMap *m);
const char *nlb_sourcemap_build(const nlb_SourceMap *m);

/* The source of a position: 1 with *file (valid until the map is freed)
** and *srcline set, or 0 when the map has none.
*/
int nlb_sourcemap_lookup(const nlb_SourceMap *m, const char *short_src,
			 int line, const char **file, int *srcline);

/* `text` (an error message, a traceback) with every "chunk:line" of a
** chunk of the maps replaced by its source "file:line".
*/
nlb_Text *nlb_remap(const nlb_SourceMap *const *maps, size_t nmaps,
		    const char *text, size_t size);

/* For maps of nlb_sourcemap_parse and nlb_take_sourcemap. */
void nlb_sourcemap_free(nlb_SourceMap *m);

#ifdef __cplusplus
}
#endif

#endif
