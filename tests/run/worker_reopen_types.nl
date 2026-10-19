# A type declared again in a reopened module is another type, and only the first has a name another state finds
fn echo(v: any): any
  v
end

module A
  enum E
    First
  end
  struct S
    a: Number
  end
  fn first_e
    E
  end
  fn first_s
    S { a: 1 }
  end
end

module A
  enum E
    Second
  end
  struct S
    b: Number
  end
  fn second_e
    E
  end
  fn second_s
    S { b: 2 }
  end
end

async fn main
  let e = A.first_e
  let back = spawn echo(e).await
  print Table.concat(back.values, ","), back == e
  let s = A.first_s
  let sb = spawn echo(s).await
  print getmetatable(sb) == getmetatable(s), sb.a
  for v in [A.second_e, A.second_s]
    try
      spawn echo(v).await
      print "sent"
    catch err
      print typeof(err), err.message
    end
  end
end

main() unless Worker.parent
