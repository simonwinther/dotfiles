# What you're about to see below, is highly confidential and should not be
# shared with anyone outside of the inner circle of zsh function developers.
# Just kidding. The following below is a standard way to autoload all zsh
# functions placed in the same directory as this init script.
# Effectively it is the same as doing this:
# fpath=(${0:A:h} $fpath)
# autoload -Uz ez v vf gcd tldrf
# It is just more maintainable this way.

# Define the directory and add to fpath
local func_dir="${0:A:h}"
fpath=($func_dir $fpath)
typeset -U fpath

# Get the list of all function files (excluding this init file)
local files=($func_dir/*(N:t))
local funcs=(${files:#init.zsh})

# Forget the old versions so edits take effect on reload
for f in $funcs; do
  unfunction $f 2>/dev/null
done

# Mark them for lazy-loading
if (( ${#funcs} )); then
  autoload -Uz $funcs
fi
