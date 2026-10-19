# A struct, enum, interface or module declared inside a function has no name another state finds, on the stand-in as on the fork
fn echo(v: any): any
  v
end

fn local_struct
  struct LS
    a: Number
  end
  LS { a: 1 }
end

fn local_enum
  enum LE
    A
  end
  LE
end

fn local_iface
  interface LI
    fn f: Number
  end
  LI
end

fn local_module
  module LM
    fn f: Number
      1
    end
  end
  LM
end

async fn main
  for t in [local_struct(), local_enum(), local_iface(), local_module()]
    try
      spawn echo(t).await
      print "sent"
    catch e
      print typeof(e), e.message
    end
  end
end

main() unless Worker.parent
