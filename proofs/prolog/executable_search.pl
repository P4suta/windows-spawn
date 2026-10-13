% SPDX-FileCopyrightText: 2026 Yasunobu Sakashita
% SPDX-License-Identifier: MIT OR Apache-2.0

:- module(executable_search,
          [ resolve/3,
            mistake/3,
            host_directory/2,
            split_paths/2
          ]).

/** <module> The search that turns a command's program into the path CreateProcessW receives

docs/crate.md promises that lookup follows the safe Windows search of Rust's standard library.
This module states that search as relations,
from the documentation and source of `std::process::Command` and `std::env::split_paths`,
apart from the crate's code.
oracle.pl evaluates it over a grid of scenarios,
and a Windows test runs the crate's search on each scenario.

A design is `documented`, or a mistake that changes one rule of the documented search.
The oracle requires every mistake to change some row,
so the rows tell each mistake apart from the documented search.
*/

:- use_module(library(apply)).
:- use_module(library(lists)).

%!  host_directory(?Name, ?Path) is nondet.
%
%   The directories the search reads from the system, the same in every scenario.

host_directory(application, 'C:\\app').
host_directory(system, 'C:\\Windows\\System32').
host_directory(windows, 'C:\\Windows').

%!  mistake(?Design, ?Rule, ?Variant) is nondet.
%
%   Each mistaken design replaces the documented variant of one rule.

mistake(native_create_process, order, native).
mistake(parent_before_system, order, parent_first).
mistake(ignores_child_path, order, without_child).
mistake(keeps_empty_entries, empty_entries, kept).
mistake(appends_exe_to_any_extension, exe, unless_exe_suffix).
mistake(never_appends_exe, exe, never).
mistake(searches_sub_paths, sub_paths, searched).
mistake(case_sensitive_suffix, suffix, case_sensitive).

rule(Design, Rule, Variant) :-
    (   mistake(Design, Rule, Mistaken)
    ->  Variant = Mistaken
    ;   Variant = documented
    ).

%!  resolve(+Design, +Scenario, -Result) is det.
%
%   Scenario is scenario(Program, ChildPath, ParentPath, Files):
%   each PATH is `unset` or set(Text), and Files lists the paths that exist.
%   Result is path(Path), the path CreateProcessW receives, or `not_found`.

resolve(Design, scenario(Program, Child, Parent, Files), Result) :-
    (   file_name(Program)
    ->  search(Design, Program, Child, Parent, Files, Result)
    ;   sub_path(Design, Program, Child, Files, Result)
    ).

%   A program with a directory is never searched for.
%   It names a `.exe` file as it is,
%   and otherwise gains `.exe` when that file exists.

sub_path(Design, Program, Child, Files, Result) :-
    (   exe_suffix(Design, Program)
    ->  Result = path(Program)
    ;   atom_concat(Program, '.exe', Appended),
        (   exists(Appended, Files)
        ->  Result = path(Appended)
        ;   rule(Design, sub_paths, searched),
            path_entries(Design, Child, Directories),
            member(Directory, Directories),
            push(Directory, Appended, Candidate),
            exists(Candidate, Files)
        ->  Result = path(Candidate)
        ;   Result = path(Program)
        )
    ).

%   A bare file name is looked for in each directory in turn, and the first file that exists wins.

search(Design, Program, Child, Parent, Files, Result) :-
    directories(Design, Child, Parent, Directories),
    (   member(Directory, Directories),
        candidate(Design, Directory, Program, Candidate),
        exists(Candidate, Files)
    ->  Result = path(Candidate)
    ;   Result = not_found
    ).

%   The child's PATH when the command changed it, the application's directory,
%   the system directory, the Windows directory, and this process's PATH.
%   The current directory is never searched.

directories(Design, Child, Parent, Directories) :-
    path_entries(Design, Child, ChildEntries),
    path_entries(Design, Parent, ParentEntries),
    host_directory(application, Application),
    host_directory(system, System),
    host_directory(windows, Windows),
    rule(Design, order, Order),
    order(Order, ChildEntries, Application, [System, Windows], ParentEntries, Directories).

order(documented, Child, Application, Systems, Parent, Directories) :-
    append([Child, [Application], Systems, Parent], Directories).
order(native, _, Application, Systems, Parent, Directories) :-
    append([[Application, '.'], Systems, Parent], Directories).
order(parent_first, Child, Application, Systems, Parent, Directories) :-
    append([Child, [Application], Parent, Systems], Directories).
order(without_child, _, Application, Systems, Parent, Directories) :-
    append([[Application], Systems, Parent], Directories).

%   A PATH yields its entries, without the empty ones.

path_entries(_, unset, []).
path_entries(Design, set(Text), Entries) :-
    split_paths(Text, All),
    (   rule(Design, empty_entries, kept)
    ->  Entries = All
    ;   exclude(==(''), All, Entries)
    ).

%   The directory joined with the program, and `.exe` when the program has no dot at all.

candidate(Design, Directory, Program, Candidate) :-
    push(Directory, Program, Joined),
    rule(Design, exe, Variant),
    (   appends_exe(Variant, Design, Program)
    ->  atom_concat(Joined, '.exe', Candidate)
    ;   Candidate = Joined
    ).

appends_exe(documented, _, Program) :-
    \+ sub_atom(Program, _, _, _, '.').
appends_exe(unless_exe_suffix, Design, Program) :-
    \+ exe_suffix(Design, Program).
appends_exe(never, _, _) :-
    fail.

%!  push(+Directory, +Name, -Joined) is det.
%
%   `PathBuf::push` of a relative name on Windows:
%   a separator goes between them unless the directory ends with one or is a bare drive like `C:`.

push(Directory, Name, Joined) :-
    (   Directory == ''
    ->  Joined = Name
    ;   drive(Directory)
    ->  atom_concat(Directory, Name, Joined)
    ;   sub_atom(Directory, _, 1, 0, Last),
        separator(Last)
    ->  atom_concat(Directory, Name, Joined)
    ;   atomic_list_concat([Directory, '\\', Name], Joined)
    ).

separator('\\').
separator('/').

drive(Path) :-
    atom_length(Path, 2),
    drive_prefix(Path).

drive_prefix(Path) :-
    sub_atom(Path, 0, 1, _, Letter),
    sub_atom(Path, 1, 1, _, ':'),
    char_code(Letter, Code),
    (   between(0'a, 0'z, Code)
    ->  true
    ;   between(0'A, 0'Z, Code)
    ).

%!  file_name(+Program) is semidet.
%
%   True if `Path::new(Program).file_name()` is Program itself:
%   no separator, no drive, and neither `.` nor `..`.

file_name(Program) :-
    Program \== '',
    Program \== '.',
    Program \== '..',
    \+ sub_atom(Program, _, _, _, '\\'),
    \+ sub_atom(Program, _, _, _, '/'),
    \+ drive_prefix(Program).

%   The program ends with `.exe` in any ASCII case.

exe_suffix(Design, Program) :-
    sub_atom(Program, _, 4, 0, Suffix),
    rule(Design, suffix, Variant),
    (   Variant == case_sensitive
    ->  Suffix == '.exe'
    ;   downcase_atom(Suffix, '.exe')
    ).

%   A path exists when one of the files matches it, as the file system matches names:
%   in any case, and with a slash read as a backslash.

exists(Path, Files) :-
    normal(Path, Normal),
    member(File, Files),
    normal(File, Normal),
    !.

normal(Path, Normal) :-
    downcase_atom(Path, Lower),
    atomic_list_concat(Parts, '/', Lower),
    atomic_list_concat(Parts, '\\', Normal).

%!  split_paths(+Text, -Entries) is det.
%
%   `std::env::split_paths` on Windows:
%   a semicolon outside double quotes ends an entry, the quotes themselves vanish,
%   and the text always yields at least one entry.

split_paths(Text, Entries) :-
    atom_codes(Text, Codes),
    split(Codes, outside, [], Entries).

split([], _, Current, [Entry]) :-
    entry(Current, Entry).
split([0'"|Codes], Quote, Current, Entries) :-
    !,
    toggle(Quote, Next),
    split(Codes, Next, Current, Entries).
split([0';|Codes], outside, Current, [Entry|Entries]) :-
    !,
    entry(Current, Entry),
    split(Codes, outside, [], Entries).
split([Code|Codes], Quote, Current, Entries) :-
    split(Codes, Quote, [Code|Current], Entries).

entry(Reversed, Entry) :-
    reverse(Reversed, Codes),
    atom_codes(Entry, Codes).

toggle(outside, inside).
toggle(inside, outside).

:- begin_tests(executable_search).

test(quoted_semicolons_stay_in_their_entry) :-
    split_paths('c:\\foo;c:\\som"e;di"r;c:\\bar', Entries),
    Entries == ['c:\\foo', 'c:\\some;dir', 'c:\\bar'].

test(every_text_yields_an_entry) :-
    split_paths('', Empty),
    Empty == [''],
    split_paths(';', Two),
    Two == ['', ''].

test(a_bare_drive_takes_no_separator) :-
    push('C:', 'tool.exe', Drive),
    Drive == 'C:tool.exe',
    push('C:\\', 'tool.exe', Root),
    Root == 'C:\\tool.exe',
    push('C:/fwd', 'tool.exe', Slash),
    Slash == 'C:/fwd\\tool.exe'.

test(the_child_path_comes_first) :-
    resolve(documented,
            scenario(tool, set('C:\\child'), set('C:\\parent'),
                     ['C:\\parent\\tool.exe', 'C:\\Windows\\System32\\tool.exe', 'C:\\child\\TOOL.EXE']),
            Result),
    Result == path('C:\\child\\tool.exe').

test(the_system_directories_come_before_the_parent_path) :-
    resolve(documented,
            scenario(tool, unset, set('C:\\parent'),
                     ['C:\\parent\\tool.exe', 'C:\\Windows\\tool.exe']),
            Result),
    Result == path('C:\\Windows\\tool.exe').

test(the_current_directory_is_not_searched) :-
    resolve(documented, scenario(tool, unset, unset, ['.\\tool.exe']), Result),
    Result == not_found.

test(a_name_with_a_dot_keeps_it) :-
    resolve(documented, scenario('tool.cmd', set('C:\\child'), unset, ['C:\\child\\tool.cmd']), Result),
    Result == path('C:\\child\\tool.cmd').

test(a_sub_path_gains_exe_only_when_that_file_exists) :-
    resolve(documented, scenario('dir\\tool', unset, unset, ['dir\\tool.exe']), Found),
    Found == path('dir\\tool.exe'),
    resolve(documented, scenario('dir\\tool', set('C:\\child'), unset, ['C:\\child\\dir\\tool.exe']), Kept),
    Kept == path('dir\\tool').

:- end_tests(executable_search).
