# import { names } from 'path'; imported names keep their binding kinds and types
import { square, answer, limit, Mode, Point, Pair, Tools } from 'import/lib'
print square(3)
print answer
print limit
print Mode.Hard
let p = Point { x: 1, y: 2 }
print p.sum
print(p is Point)
let pr: Pair = {a: 1, b: 2}
print pr.a + pr.b
print Tools.twice(4)
