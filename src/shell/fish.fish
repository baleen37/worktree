function wt
    set -l wt_path_file (mktemp)
    or return 1
    set -lx WT_SHELL_PATH_FILE "$wt_path_file"
    command wt $argv
    set -l wt_status $status
    if test $wt_status -eq 0
        set -l wt_path (string collect < "$wt_path_file")
        if test -n "$wt_path"
            builtin cd -- "$wt_path"
            or set wt_status $status
        end
    end
    command rm -f -- "$wt_path_file"
    return $wt_status
end
