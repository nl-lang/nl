# The worker example: spawns jobs with a host type that crosses both ways.
import { square, scaled, describe, fail, where } from 'jobs'

pub fn triple(x) x * 3 end

pub async fn run(v)
  let out = []
  out[out.len + 1] = "square #{spawn square(7).await}"
  let tasks = []
  for i in 1..10
    tasks[i] = spawn square(i)
  end
  let sum = 0
  for x in Task.all(tasks).await
    sum += x
  end
  out[out.len + 1] = "sum #{sum}"
  let w = spawn scaled(v, 3).await
  out[out.len + 1] = "scaled #{typeof(w)} #{:vec2_str(w)}"
  out[out.len + 1] = spawn describe(v).await
  try
    spawn fail("bad job").await
  catch e
    out[out.len + 1] = "caught #{typeof(e)}: #{e.message}"
  end
  out[out.len + 1] = "jobs run in #{spawn where().await}, this in #{where()}"
  out
end
