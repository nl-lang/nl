# Loading a file again replaces its handlers (the unnamed handler is named by the file path)
include("include/handler.nl")
include("include/handler.nl")
Event.run "Reloaded"
