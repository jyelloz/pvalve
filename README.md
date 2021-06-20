# pvalve: Pipe Valve

```svgbob
        _._._._
       (_|_|_|_)
         ` * `
          /|\
          |||
==========O+O=========
:::::::::: + . . . . .
======================
```

Copy stdin to stdout with control over the throughput limit.

The basic idea is the same as the well-known tool PV, except this tool allows
the user to pause/resume and modify the throughput -- including the units --
while the transfer is running with an interactive, full-screen text user
interface in addition to monitoring progress.
