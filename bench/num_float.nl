# Float arithmetic: the Mandelbrot set on a grid, counting the iterations
# of every point (+ - * and comparisons on doubles, nested while loops).
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const size = 1000
const max_iter = 100

let total = 0
for _rep in 1..scale
  for py in 0...size
    let ci = py * 2.0 / size - 1.0
    for px in 0...size
      let cr = px * 2.5 / size - 2.0
      let zr = 0.0
      let zi = 0.0
      let n = 0
      while n < max_iter and zr * zr + zi * zi <= 4.0
        let t = zr * zr - zi * zi + cr
        zi = 2.0 * zr * zi + ci
        zr = t
        n += 1
      end
      total += n
    end
  end
end
print total
