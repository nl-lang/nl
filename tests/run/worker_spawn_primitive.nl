# spawn of a method on a primitive type called through the type: the first argument is self
fn String.shout
  self.upcase <> "!"
end

fn Number.mul(n)
  self * n
end

async fn main
  let t = spawn String.shout("job")
  print t.await
  print spawn Number.mul(6, 7).await
end

main() unless Worker.parent
