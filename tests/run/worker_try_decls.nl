# A spawn do block may read what the body or the finally of a top-level try declares, which runs once when the file loads
try
  struct Box
    v: Number
  end
  module M
    fn sib(n: Number): Number
      n + 1
    end
    fn run(n: Number)
      spawn do
        sib(n)
      end
    end
  end
  fn make(n: Number)
    spawn do
      Box { v: n }.v * 10
    end
  end
  try
    let _ready = true
  finally
    struct Pair
      a: Number
      b: Number
    end
    fn pair(n: Number)
      spawn do
        let p = Pair { a: n, b: n + 1 }
        p.a + p.b
      end
    end
    async fn main
      print "module: #{M.run(1).await}"
      print "struct: #{make(4).await}"
      print "finally: #{pair(2).await}"
    end
    main() unless Worker.parent
  end
catch e
  print "caught #{e}"
end
