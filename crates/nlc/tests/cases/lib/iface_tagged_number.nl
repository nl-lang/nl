# Helper for iface_failed_impl.nl: implements Tagged for Number, from a file
# of its own.
import { Tagged } from 'iface_tagged'
interface Tagged for Number
  fn label: String
    "label from the library"
  end
  fn tag: String
    "tag from the library"
  end
end
