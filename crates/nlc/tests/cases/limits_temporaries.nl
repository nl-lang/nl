# Finding 2: compiler temporaries do not accumulate at block level.
# Each destructuring `let` used to leave a temporary per pattern step
# in the enclosing block (7 per line here); 40 lines overflowed.
let t = [1, 2, 3]
let [a0, b0, c0] = t
let [a1, b1, c1] = t
let [a2, b2, c2] = t
let [a3, b3, c3] = t
let [a4, b4, c4] = t
let [a5, b5, c5] = t
let [a6, b6, c6] = t
let [a7, b7, c7] = t
let [a8, b8, c8] = t
let [a9, b9, c9] = t
let [a10, b10, c10] = t
let [a11, b11, c11] = t
let [a12, b12, c12] = t
let [a13, b13, c13] = t
let [a14, b14, c14] = t
let [a15, b15, c15] = t
let [a16, b16, c16] = t
let [a17, b17, c17] = t
let [a18, b18, c18] = t
let [a19, b19, c19] = t
let [a20, b20, c20] = t
let [a21, b21, c21] = t
let [a22, b22, c22] = t
let [a23, b23, c23] = t
let [a24, b24, c24] = t
let [a25, b25, c25] = t
let [a26, b26, c26] = t
let [a27, b27, c27] = t
let [a28, b28, c28] = t
let [a29, b29, c29] = t
let [a30, b30, c30] = t
let [a31, b31, c31] = t
let [a32, b32, c32] = t
let [a33, b33, c33] = t
let [a34, b34, c34] = t
let [a35, b35, c35] = t
let [a36, b36, c36] = t
let [a37, b37, c37] = t
let [a38, b38, c38] = t
let [a39, b39, c39] = t
print a0 + b0 + c0, c39
fn inner
  let {x: x0, y: [y0, z0]} = {x: 0, y: [1, 2]}
  let {x: x1, y: [y1, z1]} = {x: 1, y: [1, 2]}
  let {x: x2, y: [y2, z2]} = {x: 2, y: [1, 2]}
  let {x: x3, y: [y3, z3]} = {x: 3, y: [1, 2]}
  let {x: x4, y: [y4, z4]} = {x: 4, y: [1, 2]}
  let {x: x5, y: [y5, z5]} = {x: 5, y: [1, 2]}
  let {x: x6, y: [y6, z6]} = {x: 6, y: [1, 2]}
  let {x: x7, y: [y7, z7]} = {x: 7, y: [1, 2]}
  let {x: x8, y: [y8, z8]} = {x: 8, y: [1, 2]}
  let {x: x9, y: [y9, z9]} = {x: 9, y: [1, 2]}
  let {x: x10, y: [y10, z10]} = {x: 10, y: [1, 2]}
  let {x: x11, y: [y11, z11]} = {x: 11, y: [1, 2]}
  let {x: x12, y: [y12, z12]} = {x: 12, y: [1, 2]}
  let {x: x13, y: [y13, z13]} = {x: 13, y: [1, 2]}
  let {x: x14, y: [y14, z14]} = {x: 14, y: [1, 2]}
  let {x: x15, y: [y15, z15]} = {x: 15, y: [1, 2]}
  let {x: x16, y: [y16, z16]} = {x: 16, y: [1, 2]}
  let {x: x17, y: [y17, z17]} = {x: 17, y: [1, 2]}
  let {x: x18, y: [y18, z18]} = {x: 18, y: [1, 2]}
  let {x: x19, y: [y19, z19]} = {x: 19, y: [1, 2]}
  let {x: x20, y: [y20, z20]} = {x: 20, y: [1, 2]}
  let {x: x21, y: [y21, z21]} = {x: 21, y: [1, 2]}
  let {x: x22, y: [y22, z22]} = {x: 22, y: [1, 2]}
  let {x: x23, y: [y23, z23]} = {x: 23, y: [1, 2]}
  let {x: x24, y: [y24, z24]} = {x: 24, y: [1, 2]}
  let {x: x25, y: [y25, z25]} = {x: 25, y: [1, 2]}
  let {x: x26, y: [y26, z26]} = {x: 26, y: [1, 2]}
  let {x: x27, y: [y27, z27]} = {x: 27, y: [1, 2]}
  let {x: x28, y: [y28, z28]} = {x: 28, y: [1, 2]}
  let {x: x29, y: [y29, z29]} = {x: 29, y: [1, 2]}
  let {x: x30, y: [y30, z30]} = {x: 30, y: [1, 2]}
  let {x: x31, y: [y31, z31]} = {x: 31, y: [1, 2]}
  let {x: x32, y: [y32, z32]} = {x: 32, y: [1, 2]}
  let {x: x33, y: [y33, z33]} = {x: 33, y: [1, 2]}
  let {x: x34, y: [y34, z34]} = {x: 34, y: [1, 2]}
  let {x: x35, y: [y35, z35]} = {x: 35, y: [1, 2]}
  let {x: x36, y: [y36, z36]} = {x: 36, y: [1, 2]}
  let {x: x37, y: [y37, z37]} = {x: 37, y: [1, 2]}
  let {x: x38, y: [y38, z38]} = {x: 38, y: [1, 2]}
  let {x: x39, y: [y39, z39]} = {x: 39, y: [1, 2]}
  x39 + y39 + z39
end
print inner()
# One table literal with 205 calls before an item that needs statements:
# the earlier items are spilled into one table, not 205 locals.
let c = true
let big = [tostring(0), tostring(1), tostring(2), tostring(3), tostring(4), tostring(5), tostring(6), tostring(7), tostring(8), tostring(9), tostring(10), tostring(11), tostring(12), tostring(13), tostring(14), tostring(15), tostring(16), tostring(17), tostring(18), tostring(19), tostring(20), tostring(21), tostring(22), tostring(23), tostring(24), tostring(25), tostring(26), tostring(27), tostring(28), tostring(29), tostring(30), tostring(31), tostring(32), tostring(33), tostring(34), tostring(35), tostring(36), tostring(37), tostring(38), tostring(39), tostring(40), tostring(41), tostring(42), tostring(43), tostring(44), tostring(45), tostring(46), tostring(47), tostring(48), tostring(49), tostring(50), tostring(51), tostring(52), tostring(53), tostring(54), tostring(55), tostring(56), tostring(57), tostring(58), tostring(59), tostring(60), tostring(61), tostring(62), tostring(63), tostring(64), tostring(65), tostring(66), tostring(67), tostring(68), tostring(69), tostring(70), tostring(71), tostring(72), tostring(73), tostring(74), tostring(75), tostring(76), tostring(77), tostring(78), tostring(79), tostring(80), tostring(81), tostring(82), tostring(83), tostring(84), tostring(85), tostring(86), tostring(87), tostring(88), tostring(89), tostring(90), tostring(91), tostring(92), tostring(93), tostring(94), tostring(95), tostring(96), tostring(97), tostring(98), tostring(99), tostring(100), tostring(101), tostring(102), tostring(103), tostring(104), tostring(105), tostring(106), tostring(107), tostring(108), tostring(109), tostring(110), tostring(111), tostring(112), tostring(113), tostring(114), tostring(115), tostring(116), tostring(117), tostring(118), tostring(119), tostring(120), tostring(121), tostring(122), tostring(123), tostring(124), tostring(125), tostring(126), tostring(127), tostring(128), tostring(129), tostring(130), tostring(131), tostring(132), tostring(133), tostring(134), tostring(135), tostring(136), tostring(137), tostring(138), tostring(139), tostring(140), tostring(141), tostring(142), tostring(143), tostring(144), tostring(145), tostring(146), tostring(147), tostring(148), tostring(149), tostring(150), tostring(151), tostring(152), tostring(153), tostring(154), tostring(155), tostring(156), tostring(157), tostring(158), tostring(159), tostring(160), tostring(161), tostring(162), tostring(163), tostring(164), tostring(165), tostring(166), tostring(167), tostring(168), tostring(169), tostring(170), tostring(171), tostring(172), tostring(173), tostring(174), tostring(175), tostring(176), tostring(177), tostring(178), tostring(179), tostring(180), tostring(181), tostring(182), tostring(183), tostring(184), tostring(185), tostring(186), tostring(187), tostring(188), tostring(189), tostring(190), tostring(191), tostring(192), tostring(193), tostring(194), tostring(195), tostring(196), tostring(197), tostring(198), tostring(199), tostring(200), tostring(201), tostring(202), tostring(203), tostring(204), if c
  "last"
end]
print big.len, big[1], big[205], big[206]
