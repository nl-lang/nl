# Methods, self, @field, closures that see self, and Self as a constructor
struct Counter
  count: Number = 0
  step: Number = 1
end

fn Counter.bump
  @count += @step
end

fn Counter.add(n)
  self.count = self.count + n
end

fn Counter.add_all(list)
  each list do |v|
    @count += v
  end
end

fn Counter.fresh: Self
  Self { step: 10 }
end

fn each(list: any, f: Function)
  for v in list
    f(v)
  end
end

let c = Counter { count: 0 }
c.bump
c.bump
print c.count
c.add 5
print c.count
c.add_all([1, 2, 3])
print c.count
let f = Counter.fresh
print f.step
print typeof(f)
f.bump
print f.count
