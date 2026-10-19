# Finding 2: more than 200 file-level declarations, and a function that
# calls 70 file-level functions, fit LuaJIT's limits (200 locals, 60
# upvalues per function): the file-level locals move to a hidden table.
let v0 = 0
let v1 = 1
let v2 = 2
let v3 = 3
let v4 = 4
let v5 = 5
let v6 = 6
let v7 = 7
let v8 = 8
let v9 = 9
let v10 = 10
let v11 = 11
let v12 = 12
let v13 = 13
let v14 = 14
let v15 = 15
let v16 = 16
let v17 = 17
let v18 = 18
let v19 = 19
let v20 = 20
let v21 = 21
let v22 = 22
let v23 = 23
let v24 = 24
let v25 = 25
let v26 = 26
let v27 = 27
let v28 = 28
let v29 = 29
let v30 = 30
let v31 = 31
let v32 = 32
let v33 = 33
let v34 = 34
let v35 = 35
let v36 = 36
let v37 = 37
let v38 = 38
let v39 = 39
let v40 = 40
let v41 = 41
let v42 = 42
let v43 = 43
let v44 = 44
let v45 = 45
let v46 = 46
let v47 = 47
let v48 = 48
let v49 = 49
let v50 = 50
let v51 = 51
let v52 = 52
let v53 = 53
let v54 = 54
let v55 = 55
let v56 = 56
let v57 = 57
let v58 = 58
let v59 = 59
let v60 = 60
let v61 = 61
let v62 = 62
let v63 = 63
let v64 = 64
let v65 = 65
let v66 = 66
let v67 = 67
let v68 = 68
let v69 = 69
let v70 = 70
let v71 = 71
let v72 = 72
let v73 = 73
let v74 = 74
let v75 = 75
let v76 = 76
let v77 = 77
let v78 = 78
let v79 = 79
let v80 = 80
let v81 = 81
let v82 = 82
let v83 = 83
let v84 = 84
let v85 = 85
let v86 = 86
let v87 = 87
let v88 = 88
let v89 = 89
let v90 = 90
let v91 = 91
let v92 = 92
let v93 = 93
let v94 = 94
let v95 = 95
let v96 = 96
let v97 = 97
let v98 = 98
let v99 = 99
let v100 = 100
let v101 = 101
let v102 = 102
let v103 = 103
let v104 = 104
let v105 = 105
let v106 = 106
let v107 = 107
let v108 = 108
let v109 = 109
let v110 = 110
let v111 = 111
let v112 = 112
let v113 = 113
let v114 = 114
let v115 = 115
let v116 = 116
let v117 = 117
let v118 = 118
let v119 = 119
let v120 = 120
let v121 = 121
let v122 = 122
let v123 = 123
let v124 = 124
let v125 = 125
let v126 = 126
let v127 = 127
let v128 = 128
let v129 = 129
let v130 = 130
let v131 = 131
let v132 = 132
let v133 = 133
let v134 = 134
let v135 = 135
let v136 = 136
let v137 = 137
let v138 = 138
let v139 = 139
let v140 = 140
let v141 = 141
let v142 = 142
let v143 = 143
let v144 = 144
let v145 = 145
let v146 = 146
let v147 = 147
let v148 = 148
let v149 = 149
let v150 = 150
let v151 = 151
let v152 = 152
let v153 = 153
let v154 = 154
let v155 = 155
let v156 = 156
let v157 = 157
let v158 = 158
let v159 = 159
let v160 = 160
let v161 = 161
let v162 = 162
let v163 = 163
let v164 = 164
let v165 = 165
let v166 = 166
let v167 = 167
let v168 = 168
let v169 = 169
let v170 = 170
let v171 = 171
let v172 = 172
let v173 = 173
let v174 = 174
let v175 = 175
let v176 = 176
let v177 = 177
let v178 = 178
let v179 = 179
let v180 = 180
let v181 = 181
let v182 = 182
let v183 = 183
let v184 = 184
let v185 = 185
let v186 = 186
let v187 = 187
let v188 = 188
let v189 = 189
let v190 = 190
let v191 = 191
let v192 = 192
let v193 = 193
let v194 = 194
let v195 = 195
let v196 = 196
let v197 = 197
let v198 = 198
let v199 = 199
let v200 = 200
let v201 = 201
let v202 = 202
let v203 = 203
let v204 = 204
let v205 = 205
let v206 = 206
let v207 = 207
let v208 = 208
let v209 = 209
fn f0
  0
end
fn f1
  1
end
fn f2
  2
end
fn f3
  3
end
fn f4
  4
end
fn f5
  5
end
fn f6
  6
end
fn f7
  7
end
fn f8
  8
end
fn f9
  9
end
fn f10
  10
end
fn f11
  11
end
fn f12
  12
end
fn f13
  13
end
fn f14
  14
end
fn f15
  15
end
fn f16
  16
end
fn f17
  17
end
fn f18
  18
end
fn f19
  19
end
fn f20
  20
end
fn f21
  21
end
fn f22
  22
end
fn f23
  23
end
fn f24
  24
end
fn f25
  25
end
fn f26
  26
end
fn f27
  27
end
fn f28
  28
end
fn f29
  29
end
fn f30
  30
end
fn f31
  31
end
fn f32
  32
end
fn f33
  33
end
fn f34
  34
end
fn f35
  35
end
fn f36
  36
end
fn f37
  37
end
fn f38
  38
end
fn f39
  39
end
fn f40
  40
end
fn f41
  41
end
fn f42
  42
end
fn f43
  43
end
fn f44
  44
end
fn f45
  45
end
fn f46
  46
end
fn f47
  47
end
fn f48
  48
end
fn f49
  49
end
fn f50
  50
end
fn f51
  51
end
fn f52
  52
end
fn f53
  53
end
fn f54
  54
end
fn f55
  55
end
fn f56
  56
end
fn f57
  57
end
fn f58
  58
end
fn f59
  59
end
fn f60
  60
end
fn f61
  61
end
fn f62
  62
end
fn f63
  63
end
fn f64
  64
end
fn f65
  65
end
fn f66
  66
end
fn f67
  67
end
fn f68
  68
end
fn f69
  69
end
fn all
  let s = 0
  s += f0
  s += f1
  s += f2
  s += f3
  s += f4
  s += f5
  s += f6
  s += f7
  s += f8
  s += f9
  s += f10
  s += f11
  s += f12
  s += f13
  s += f14
  s += f15
  s += f16
  s += f17
  s += f18
  s += f19
  s += f20
  s += f21
  s += f22
  s += f23
  s += f24
  s += f25
  s += f26
  s += f27
  s += f28
  s += f29
  s += f30
  s += f31
  s += f32
  s += f33
  s += f34
  s += f35
  s += f36
  s += f37
  s += f38
  s += f39
  s += f40
  s += f41
  s += f42
  s += f43
  s += f44
  s += f45
  s += f46
  s += f47
  s += f48
  s += f49
  s += f50
  s += f51
  s += f52
  s += f53
  s += f54
  s += f55
  s += f56
  s += f57
  s += f58
  s += f59
  s += f60
  s += f61
  s += f62
  s += f63
  s += f64
  s += f65
  s += f66
  s += f67
  s += f68
  s += f69
  s
end
print v0, v209, all
v209 += 1
print v209
