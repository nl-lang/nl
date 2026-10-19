# `fn X.m` needs X to be a struct or module; an imported name is
# judged by what declares it in its file.
import { Mode, Box } from 'lib/kinds'
fn Box.ok
  1
end
# expect-error: `Mode` is an imported enum
fn Mode.bad
  1
end
