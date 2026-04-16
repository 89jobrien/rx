#!/usr/bin/env fish

set name world
set index (contains -i -- --name $argv)
if test -n "$index"
    set next (math $index + 1)
    if test $next -le (count $argv)
        set name $argv[$next]
    end
end

echo "hello from fish, $name!"
