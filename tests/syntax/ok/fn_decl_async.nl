# `async fn` declares an async function.
async fn load_profile(id)
  http_get(id).await
end
