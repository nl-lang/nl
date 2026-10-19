# Recovery: tokens that cannot start an operand, mixed with prefix operators,
# are skipped as one error node; the operand after them is read.
x = - * - * y
z = 2
