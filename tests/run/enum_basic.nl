# Enums, their values, membership, values and name
enum Direction
  North
  East
  South
  West
end

enum Team
  Red = 1
  Blue = 2
end

enum Mode
  Easy = "easy"
  Hard = "hard"
end

let d = Direction.North
print d
print(d == "North")
print Team.Red
print Mode.Hard
print(d is Direction)
print("Up" is Direction)
print(2 is Team)
print(3 is Team)
print Table.concat(Direction.values, ",")
print Direction.values.len
print Team.name(2)
print Direction.name("West")
print Mode.name("easy")

fn turn(dir: Direction)
  case dir
  when Direction.North
    "going north"
  when Direction.East, Direction.West
    "sideways"
  else
    "south"
  end
end

print turn(Direction.West)
print turn(Direction.North)
let bad: any = "Up"
try
  turn(bad)
catch err: TypeError
  print "TypeError"
end
