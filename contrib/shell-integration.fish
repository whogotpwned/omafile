# Fish integration for omafile. Copy to ~/.config/fish/functions/of.fish
function of --description "omafile, cd to the directory on exit"
    set -l tmp (mktemp -t omafile-cwd.XXXXXX)
    omafile --cwd-file $tmp $argv
    set -l dir (cat $tmp 2>/dev/null)
    rm -f $tmp
    if test -n "$dir" -a "$dir" != "$PWD" -a -d "$dir"
        cd $dir
    end
end
