# Jobs of the worker example (host.c, src/bin/workers.rs). No top-level
# effects: pool workers load this file.
pub fn square(x) x * x end

pub fn scaled(v, k) :vec2_scale(v, k) end

pub fn describe(v) "#{typeof(v)} #{:vec2_str(v)}" end

pub fn fail(msg) raise msg end

pub fn where
  if :host_isworker()
    "a worker"
  else
    "the main state"
  end
end
