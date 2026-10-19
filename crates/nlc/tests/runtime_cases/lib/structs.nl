struct Q
  q: Q?
end
fn Q.-(o)
  Q {}
end
Q { q: Q {} } - Q {}
