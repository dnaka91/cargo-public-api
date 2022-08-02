# Public API diffing

## How it works

This tool performs public API diffing of commits **A** and **B** by doing
1. `git checkout A`
1. Build rustdoc JSON for **A**
1. Collect all public items of the public API of **A**
1. `git checkout B`
1. Build rustdoc JSON for **B**
1. Collect all public items of the public API **B**
1. Calculate 

This has the following consequences:
* If **B** is not your current branch, you need to manually checkout


You can also manually do a diff by writing the full list of items to a file for two different versions of your library and then do a regular `diff` between the files.


