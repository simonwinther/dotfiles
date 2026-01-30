# Add this directory to fpath for autoloaded functions
fpath=(${0:A:h} $fpath)

# Autoload all functions in this dir (except init)
# This is same as, manully doing:
# fpath=(${0:A:h} $fpath)
# autoload -Uz ez v vf gcd tldrf
# Just cooler :D 
autoload -Uz ${${(M)${(f)"$(print -l ${0:A:h}/*(:t))"}:#init}#*}


