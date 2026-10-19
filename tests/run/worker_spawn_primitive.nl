# spawn of a method on a primitive type called through the type: the first argument is self
fn String.shout
  self.upcase <> "!"
end

fn Number.times(n)
  self * n
end

async fn main
  let t = spawn String.shout("job")
  print t.await
  print spawn Number.times(6, 7).await
end

main() unless Worker.parent
