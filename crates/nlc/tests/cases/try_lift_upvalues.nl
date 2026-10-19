# A closure with a `try` that already uses 56 variables of enclosing
# functions: lifting must not add upvalues past LuaJIT's
# limit.
# backends: lua,bc
fn f
  let a0 = 0
  let a1 = 1
  let a2 = 2
  let a3 = 3
  let a4 = 4
  let a5 = 5
  let a6 = 6
  let a7 = 7
  let a8 = 8
  let a9 = 9
  let a10 = 10
  let a11 = 11
  let a12 = 12
  let a13 = 13
  let a14 = 14
  let a15 = 15
  let a16 = 16
  let a17 = 17
  let a18 = 18
  let a19 = 19
  let a20 = 20
  let a21 = 21
  let a22 = 22
  let a23 = 23
  let a24 = 24
  let a25 = 25
  let a26 = 26
  let a27 = 27
  let a28 = 28
  let a29 = 29
  let a30 = 30
  let a31 = 31
  let a32 = 32
  let a33 = 33
  let a34 = 34
  let a35 = 35
  let a36 = 36
  let a37 = 37
  let a38 = 38
  let a39 = 39
  let a40 = 40
  let a41 = 41
  let a42 = 42
  let a43 = 43
  let a44 = 44
  let a45 = 45
  let a46 = 46
  let a47 = 47
  let a48 = 48
  let a49 = 49
  let a50 = 50
  let a51 = 51
  let a52 = 52
  let a53 = 53
  let a54 = 54
  let k = 0
  let g = fn()
    k = k + 1
    try
      k = k + a0 + a1 + a2 + a3 + a4 + a5 + a6 + a7 + a8 + a9 + a10 + a11 + a12 + a13 + a14 + a15 + a16 + a17 + a18 + a19 + a20 + a21 + a22 + a23 + a24 + a25 + a26 + a27 + a28 + a29 + a30 + a31 + a32 + a33 + a34 + a35 + a36 + a37 + a38 + a39 + a40 + a41 + a42 + a43 + a44 + a45 + a46 + a47 + a48 + a49 + a50 + a51 + a52 + a53 + a54
    catch e
      print "err"
    end
    k
  end
  g()
end
print f()
