# spawn and Worker.run run their jobs in this state; the results arrive through the Think pump
fn square(x: Number): Number
  x * x
end

async fn main
  let a = spawn square(7)
  let b = Worker.run(square!, 3)
  print "waiting"
  print a.await + b.await
  let m = Worker.map([1, 2, 3], square!).await
  print Table.concat(m, " ")
end
main()
print "main returned"
