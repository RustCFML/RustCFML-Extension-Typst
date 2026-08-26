# invoice

A two-page invoice built with the fluent `Document()` builder, plus
`selftest.cfm` — 25 end-to-end checks over the whole surface.

```sh
rustcfml ext build ../..                      # from this directory
rustcfml ext install ../../typst-0.1.0.rcx --dir extensions
rustcfml main.cfm
rustcfml selftest.cfm
```

The engine running those is a **stock** `rustcfml` binary. `extensions/` beside
the application is one of the directories the engine searches, so an extension
checked in here travels with the project.

`main.cfm` prints the generated Typst at the end — that is `.toTypst()`, the
debugging seam. When a document comes out wrong, read the markup rather than
guessing.
