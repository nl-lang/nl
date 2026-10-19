# Benchmark results

Status: one complete run of every pair with the final workloads and scales (bench/run.sh, 5 runs each); each pair was timed once in this run, except `str_build` JIT, which was re-timed by the 10% rule. An earlier full run (before the last rescale of `floordiv` and `bitops` JIT scales) agreed within a few percent except `floordiv` JIT, which was noisy there and is at parity here. Not re-run since.

Machine: Apple M5 Pro, 18 cores, Darwin 27.0.1. Measured 2026-10-12.

Load averages (1, 5, 15 min): 2.83 2.91 3.48 before, 2.88 3.62 3.32 after. Waited 0 s for cargo/rustc/cc/clang/make to finish before timing.

Each cell is the wall-clock time of the whole process in seconds, minimum / median of 5 runs (`bench/run.sh`). *interp* is the build with `-DLUAJIT_DISABLE_JIT`, *JIT* the default build, both the fork with the real compiler and runtime, hosted by `bench/nlbench.c`. *scale* is how many times the program repeats its workload (NLB_SCALE). The ratios divide minima; *net* ratios first subtract the minimum of the `empty` pair (startup: state, runtime, compile) of the same build and side. *NL unchecked* is the NL program compiled with NL_OPT_CHECKS=0 (`nlc build --unchecked`).

| pair | mode | scale | Lua (min / med) | NL (min / med) | NL/Lua | net | NL unchecked (min / med) | unchecked/Lua | net |
|---|---|---|---|---|---|---|---|---|---|
| empty | interp | 1 | 0.003 / 0.003 | 0.004 / 0.004 | **1.26** | - | 0.004 / 0.004 | 1.32 | - |
| empty | JIT | 1 | 0.003 / 0.004 | 0.004 / 0.004 | **1.25** | - | 0.004 / 0.004 | 1.25 | - |
| bitops | interp | 1 | 0.410 / 0.417 | 1.747 / 1.784 | **4.26** | 4.28 | 1.752 / 1.780 | 4.27 | 4.29 |
| bitops | JIT | 4 | 0.054 / 0.054 | 7.110 / 7.165 | **132.64** | 140.98 | 6.969 / 7.107 | 130.01 | 138.19 |
| case_match | interp | 1 | 0.224 / 0.230 | 1.527 / 1.544 | **6.81** | 6.89 | 1.080 / 1.081 | 4.82 | 4.86 |
| case_match | JIT | 8 | 0.232 / 0.232 | 0.294 / 0.306 | **1.27** | 1.27 | 0.311 / 0.318 | 1.34 | 1.35 |
| closures | interp | 1 | 0.495 / 0.498 | 3.027 / 3.042 | **6.12** | 6.15 | 0.498 / 0.502 | 1.01 | 1.01 |
| closures | JIT | 1 | 0.457 / 0.461 | 0.695 / 0.706 | **1.52** | 1.52 | 0.459 / 0.460 | 1.00 | 1.00 |
| fields | interp | 1 | 0.877 / 0.900 | 1.026 / 1.046 | **1.17** | 1.17 | 1.038 / 1.049 | 1.18 | 1.18 |
| fields | JIT | 10 | 0.406 / 0.411 | 0.643 / 0.655 | **1.58** | 1.59 | 0.630 / 0.637 | 1.55 | 1.55 |
| fields_typed | interp | 1 | 0.862 / 0.896 | 3.464 / 3.578 | **4.02** | 4.03 | 1.033 / 1.037 | 1.20 | 1.20 |
| fields_typed | JIT | 10 | 0.409 / 0.420 | 2.380 / 2.534 | **5.81** | 5.85 | 0.630 / 0.647 | 1.54 | 1.54 |
| floordiv | interp | 1 | 0.468 / 0.483 | 1.043 / 1.071 | **2.23** | 2.24 | 1.061 / 1.081 | 2.27 | 2.28 |
| floordiv | JIT | 6 | 0.329 / 0.333 | 0.331 / 0.334 | **1.01** | 1.00 | 0.333 / 0.334 | 1.01 | 1.01 |
| interp | interp | 1 | 0.748 / 0.757 | 0.841 / 0.848 | **1.12** | 1.12 | 0.852 / 0.854 | 1.14 | 1.14 |
| interp | JIT | 6 | 0.461 / 0.470 | 0.472 / 0.476 | **1.02** | 1.02 | 0.464 / 0.474 | 1.01 | 1.01 |
| list_index | interp | 1 | 0.882 / 0.894 | 0.881 / 0.885 | **1.00** | 1.00 | 0.884 / 0.887 | 1.00 | 1.00 |
| list_index | JIT | 2 | 0.379 / 0.381 | 0.380 / 0.384 | **1.00** | 1.00 | 0.379 / 0.381 | 1.00 | 1.00 |
| methods | interp | 1 | 0.359 / 0.363 | 0.886 / 0.897 | **2.47** | 2.48 | 0.396 / 0.397 | 1.10 | 1.10 |
| methods | JIT | 10 | 0.327 / 0.327 | 0.292 / 0.293 | **0.89** | 0.89 | 0.292 / 0.293 | 0.89 | 0.89 |
| methods_typed | interp | 1 | 0.361 / 0.370 | 3.238 / 3.297 | **8.97** | 9.04 | 0.365 / 0.369 | 1.01 | 1.01 |
| methods_typed | JIT | 10 | 0.326 / 0.327 | 0.292 / 0.292 | **0.89** | 0.89 | 0.292 / 0.292 | 0.90 | 0.89 |
| num_float | interp | 1 | 0.809 / 0.817 | 0.806 / 0.813 | **1.00** | 1.00 | 0.808 / 0.811 | 1.00 | 1.00 |
| num_float | JIT | 8 | 0.532 / 0.538 | 0.534 / 0.536 | **1.00** | 1.00 | 0.519 / 0.537 | 0.97 | 0.97 |
| num_int | interp | 1 | 0.787 / 0.789 | 0.810 / 0.816 | **1.03** | 1.03 | 0.805 / 0.815 | 1.02 | 1.02 |
| num_int | JIT | 8 | 0.590 / 0.592 | 0.591 / 0.592 | **1.00** | 1.00 | 0.590 / 0.593 | 1.00 | 1.00 |
| raytrace | interp | 1 | 0.234 / 0.236 | 3.782 / 3.836 | **16.20** | 16.40 | 0.770 / 0.775 | 3.30 | 3.32 |
| raytrace | JIT | 4 | 0.084 / 0.084 | 2.706 / 2.774 | **32.25** | 33.48 | 1.451 / 1.475 | 17.29 | 17.93 |
| str_build | interp | 1 | 1.020 / 1.038 | 1.149 / 1.181 | **1.13** | 1.13 | 1.163 / 1.186 | 1.14 | 1.14 |
| str_build | JIT (measured 2 times) | 1 | 0.645 / 0.670 | 0.663 / 0.681 | **1.03** | 1.03 | 0.635 / 0.679 | 0.99 | 0.98 |
| str_format | interp | 1 | 0.856 / 0.859 | 0.978 / 0.987 | **1.14** | 1.14 | 0.977 / 0.985 | 1.14 | 1.14 |
| str_format | JIT | 1 | 0.675 / 0.681 | 0.680 / 0.681 | **1.01** | 1.01 | 0.674 / 0.682 | 1.00 | 1.00 |
| str_methods | interp | 1 | 0.507 / 0.515 | 2.312 / 2.351 | **4.56** | 4.58 | 2.349 / 2.385 | 4.63 | 4.65 |
| str_methods | JIT | 1 | 0.325 / 0.327 | 1.044 / 1.056 | **3.21** | 3.23 | 1.038 / 1.064 | 3.20 | 3.22 |
| struct_new | interp | 1 | 0.254 / 0.255 | 4.973 / 5.032 | **19.60** | 19.83 | 1.685 / 1.733 | 6.64 | 6.71 |
| struct_new | JIT | 1 | 0.179 / 0.179 | 1.048 / 1.053 | **5.86** | 5.95 | 0.996 / 1.015 | 5.57 | 5.65 |
| try_ok † | interp | 1 | 0.105 / 0.108 | 1.769 / 1.816 | **16.85** | 17.32 | 1.382 / 1.400 | 13.16 | 13.52 |
| try_ok † | JIT | 4 | 0.049 / 0.051 | 4.852 / 4.976 | **98.23** | 104.94 | 4.877 / 4.970 | 98.72 | 105.47 |
| try_raise † | interp | 1 | 1.121 / 1.126 | 2.263 / 2.273 | **2.02** | 2.02 | 2.185 / 2.218 | 1.95 | 1.95 |
| try_raise † | JIT | 1 | 1.120 / 1.123 | 1.734 / 1.746 | **1.55** | 1.55 | 1.742 / 1.756 | 1.56 | 1.56 |

All checksums agree.

† Measured before the closures of `try` were lifted (crates/nl_ir/src/lower/lift.rs; bench/README.md, "Opcode decisions", `try`). Not re-run here: this table's builds are the fork's. After lifting, measured differently (`nlc run --backend=bc` on stock LuaJIT, level 0, minimum of 3 interleaved runs on a loaded machine, against the same Lua twins on the fork; old compiler / new compiler / Lua): `try_ok` interp 2.739 / 1.102 / 0.101 s (27.0× → 10.9×; the same loop without the `try` takes 0.88 s on level 0), `try_ok` JIT at scale 4 6.576 / 0.054 / 0.051 s (128× → 1.05×), `try_raise` interp 2.931 / 2.755 / 1.122 s (2.61× → 2.46×), `try_raise` JIT 2.334 / 2.184 / 1.126 s (2.07× → 1.94×).
