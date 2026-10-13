% SPDX-FileCopyrightText: 2026 Yasunobu Sakashita
% SPDX-License-Identifier: MIT OR Apache-2.0

/** <module> Writes the executable-search vectors that the crate's Windows test reads

Run as `swipl oracle.pl DIRECTORY`.
It writes DIRECTORY/executable_search.tsv:
a first row `#host` with the application, system, and Windows directories,
then one row per scenario with the program, the child's PATH, this process's PATH,
the files that exist, and the documented result.
A PATH is `-` when unset and `=` followed by its text when set;
the files are separated by `|`;
the result is `path=` followed by the path, or `not_found`.

It fails unless every mistaken design changes some row.
*/

:- use_module(executable_search).
:- use_module(library(apply)).
:- use_module(library(lists)).

:- initialization(main, main).

main :-
    current_prolog_flag(argv, [Directory]),
    findall(Scenario, scenario(Scenario), Scenarios),
    directory_file_path(Directory, 'executable_search.tsv', File),
    setup_call_cleanup(
        open(File, write, Stream, [encoding(utf8), newline(posix)]),
        write_vectors(Stream, Scenarios),
        close(Stream)),
    length(Scenarios, Count),
    format("~d scenarios~n", [Count]),
    forall(mistake(Design, _, _), distinguished(Design, Scenarios)).

write_vectors(Stream, Scenarios) :-
    host_directory(application, Application),
    host_directory(system, System),
    host_directory(windows, Windows),
    format(Stream, "#host\t~w\t~w\t~w~n", [Application, System, Windows]),
    forall(member(Scenario, Scenarios), write_row(Stream, Scenario)).

write_row(Stream, Scenario) :-
    Scenario = scenario(Program, Child, Parent, Files),
    resolve(documented, Scenario, Result),
    path_field(Child, ChildField),
    path_field(Parent, ParentField),
    atomic_list_concat(Files, '|', FilesField),
    result_field(Result, ResultField),
    format(Stream, "~w\t~w\t~w\t~w\t~w~n", [Program, ChildField, ParentField, FilesField, ResultField]).

path_field(unset, '-').
path_field(set(Text), Field) :-
    atom_concat('=', Text, Field).

result_field(path(Path), Field) :-
    atom_concat('path=', Path, Field).
result_field(not_found, not_found).

%   A mistake must change the result of some scenario, or the rows could not catch it.

distinguished(Design, Scenarios) :-
    aggregate_all(count,
                  ( member(Scenario, Scenarios),
                    resolve(documented, Scenario, Documented),
                    resolve(Design, Scenario, Mistaken),
                    Documented \== Mistaken
                  ),
                  Changed),
    format("~w changes ~d rows~n", [Design, Changed]),
    (   Changed > 0
    ->  true
    ;   format(user_error, "no row tells ~w from the documented search~n", [Design]),
        halt(1)
    ).

%   Bare file names meet every kind of PATH and up to two existing files among the places a search could look.
%   Programs with a directory meet up to two existing files among the names they could gain.

scenario(scenario(Program, Child, Parent, Files)) :-
    member(Program, [tool, 'Tool.EXE', 'tool.cmd']),
    member(Child, [ unset,
                    set(''),
                    set('C:\\child'),
                    set('C:\\child;;C:\\other'),
                    set('C:\\other;"C:\\quoted;dir";C:\\child'),
                    set('C:'),
                    set('C:/fwd')
                  ]),
    member(Parent, [unset, set('C:\\parent;')]),
    files([ 'C:\\child\\tool.exe',
            'C:\\other\\tool.exe',
            'C:\\quoted;dir\\tool.exe',
            'C:\\fwd\\tool.exe',
            'C:tool.exe',
            'C:\\app\\tool.exe',
            'C:\\Windows\\System32\\tool.exe',
            'C:\\Windows\\tool.exe',
            'C:\\parent\\tool.exe',
            'C:\\child\\tool.cmd',
            'C:\\child\\tool.cmd.exe',
            'C:\\child\\tool',
            '.\\tool.exe',
            'tool.exe'
          ], Files).
scenario(scenario(Program, Child, unset, Files)) :-
    member(Program, ['dir\\tool', 'dir/tool.EXE', 'C:\\abs\\tool', '.\\tool', 'C:tool']),
    member(Child, [unset, set('C:\\child')]),
    files([ 'dir\\tool.exe',
            'dir\\tool.EXE.exe',
            'C:\\abs\\tool.exe',
            '.\\tool.exe',
            'C:tool.exe',
            'C:\\child\\dir\\tool.exe'
          ], Files).

%   No file, one file, or two files in their order in the list.

files(_, []).
files(Universe, [File]) :-
    member(File, Universe).
files(Universe, [First, Second]) :-
    append(_, [First|Rest], Universe),
    member(Second, Rest).
