# A rest table sent to a worker arrives as a plain list: its count stays in this state, so trailing nils are cut
import { spread_count, arg_count, identity } from 'worker/jobs'

fn count(*args)
  :select("#", *args)
end

async fn send(*args)
  let there = spawn spread_count(args).await
  let back = spawn identity(args).await
  print "here #{count(*args)}, there #{there}, back #{count(*back)}"
end

# Spread at the call, each argument crosses as itself, nils included.
async fn forward(*args)
  let there = spawn arg_count(*args).await
  print "forwarded #{args.len}, there #{there}"
end

async fn main
  send(1, nil, nil).await
  send(1, 2).await
  forward(1, nil, nil).await
  forward().await
end

main()
