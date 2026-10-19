/* Minimal test harness for the VM tests: CHECK records a failure and goes on;
** each TEST prints one TAP-style line.
*/
#ifndef NL_TEST_CHECK_H
#define NL_TEST_CHECK_H

#include <stdio.h>
#include <string.h>

static int check_failed_total, check_count, check_test_failed, test_number;

#define CHECK(c) do { \
  check_count++; \
  if (!(c)) { \
    check_failed_total++; check_test_failed = 1; \
    fprintf(stderr, "%s:%d: CHECK failed: %s\n", __FILE__, __LINE__, #c); \
  } \
} while (0)

#define CHECK_STR(a, b) do { \
  const char *ca_ = (a), *cb_ = (b); \
  check_count++; \
  if (!ca_ || !cb_ || strcmp(ca_, cb_) != 0) { \
    check_failed_total++; check_test_failed = 1; \
    fprintf(stderr, "%s:%d: CHECK_STR failed: %s == \"%s\", expected \"%s\"\n", \
	    __FILE__, __LINE__, #a, ca_ ? ca_ : "(null)", cb_ ? cb_ : "(null)"); \
  } \
} while (0)

#define CHECK_CONTAINS(a, b) do { \
  const char *ca_ = (a), *cb_ = (b); \
  check_count++; \
  if (!ca_ || !strstr(ca_, cb_)) { \
    check_failed_total++; check_test_failed = 1; \
    fprintf(stderr, "%s:%d: CHECK_CONTAINS failed: %s == \"%s\" lacks \"%s\"\n", \
	    __FILE__, __LINE__, #a, ca_ ? ca_ : "(null)", cb_); \
  } \
} while (0)

#define RUN_TEST(fn) do { \
  check_test_failed = 0; \
  fn(); \
  printf("%s %d - %s\n", check_test_failed ? "not ok" : "ok", ++test_number, #fn); \
  fflush(stdout); \
} while (0)

static int check_summary(const char *suite)
{
  printf("1..%d\n# %s: %d checks, %d failed\n", test_number, suite,
	 check_count, check_failed_total);
  return check_failed_total ? 1 : 0;
}

#endif
