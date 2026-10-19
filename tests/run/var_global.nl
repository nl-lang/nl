# `global` declares and assigns Lua globals
global Score = 10
Score += 5
print Score
print :Score
:_G.Bonus = 7
global Bonus
print Bonus

fn bump
  Score += 1
end

bump()
print Score
