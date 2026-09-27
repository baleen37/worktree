function wt
    set -l wt_path_file (mktemp)
    or return 1
    set -lx WT_SHELL_PATH_FILE "$wt_path_file"
    command wt $argv
    set -l wt_status $status
    set -l wt_path (string collect < "$wt_path_file")
    if test -n "$wt_path"
        builtin cd -- "$wt_path"
        set -l wt_cd_status $status
        if test $wt_status -eq 0
            if test $wt_cd_status -ne 0
                set wt_status $wt_cd_status
            end
        end
    end
    command rm -f -- "$wt_path_file"
    return $wt_status
end
