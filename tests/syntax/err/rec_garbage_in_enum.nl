# Recovery: garbage inside an enum body; the variants around it parse.
enum E
  A
  1 + 2
  B = 3, C
  D
end
x = 1
