-- For the tests only: the real runtime creates no global but the prelude.
rt.host = host
__fake_rt = rt
return rt
