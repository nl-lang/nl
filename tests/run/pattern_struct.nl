# Struct patterns, and case over a union of structs
struct Circle
  r: Number
end

struct Rect
  w: Number
  h: Number
end

type Shape = Circle | Rect

fn area(shape: Shape): Number
  case shape
  when Circle {r}
    3 * r ** 2
  when Rect {w, h}
    w * h
  end
end

print area(Circle { r: 2 })
print area(Rect { w: 3, h: 5 })

let Rect {w, h: height} = Rect { w: 5, h: 6 }
print "#{w} #{height}"

fn only_rects(s)
  case s
  when Rect {w: 1..10}
    "narrow rect"
  when Rect {w}
    "wide rect #{w}"
  else
    "not a rect"
  end
end

print only_rects(Rect { w: 2, h: 1 })
print only_rects(Rect { w: 20, h: 1 })
print only_rects(Circle { r: 1 })
print only_rects({w: 2, h: 1})
