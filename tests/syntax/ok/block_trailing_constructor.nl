# A block after a callee: `Task.new do |resolve, reject| … end`.
t = Task.new do |resolve, reject|
  resolve 1
end
