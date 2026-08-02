[CmdletBinding()]
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]] $MutantsArguments = @()
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$nativeSource = @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

namespace WindowsSpawn.Tools
{
    public static class MutationJob
    {
        private const uint CreateSuspended = 0x00000004;
        private const uint KillOnJobClose = 0x00002000;
        private const int ExtendedLimitInformationClass = 9;
        private const uint Infinite = 0xffffffff;
        private const uint WaitObject0 = 0;

        [StructLayout(LayoutKind.Sequential)]
        private struct StartupInfo
        {
            internal uint cb;
            internal IntPtr lpReserved;
            internal IntPtr lpDesktop;
            internal IntPtr lpTitle;
            internal uint dwX;
            internal uint dwY;
            internal uint dwXSize;
            internal uint dwYSize;
            internal uint dwXCountChars;
            internal uint dwYCountChars;
            internal uint dwFillAttribute;
            internal uint dwFlags;
            internal ushort wShowWindow;
            internal ushort cbReserved2;
            internal IntPtr lpReserved2;
            internal IntPtr hStdInput;
            internal IntPtr hStdOutput;
            internal IntPtr hStdError;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct ProcessInformation
        {
            internal IntPtr hProcess;
            internal IntPtr hThread;
            internal uint dwProcessId;
            internal uint dwThreadId;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct BasicLimits
        {
            internal long PerProcessUserTimeLimit;
            internal long PerJobUserTimeLimit;
            internal uint LimitFlags;
            internal UIntPtr MinimumWorkingSetSize;
            internal UIntPtr MaximumWorkingSetSize;
            internal uint ActiveProcessLimit;
            internal UIntPtr Affinity;
            internal uint PriorityClass;
            internal uint SchedulingClass;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct IoCounters
        {
            internal ulong ReadOperationCount;
            internal ulong WriteOperationCount;
            internal ulong OtherOperationCount;
            internal ulong ReadTransferCount;
            internal ulong WriteTransferCount;
            internal ulong OtherTransferCount;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct ExtendedLimits
        {
            internal BasicLimits BasicLimitInformation;
            internal IoCounters IoInfo;
            internal UIntPtr ProcessMemoryLimit;
            internal UIntPtr JobMemoryLimit;
            internal UIntPtr PeakProcessMemoryUsed;
            internal UIntPtr PeakJobMemoryUsed;
        }

        [DllImport("kernel32.dll", EntryPoint = "CreateJobObjectW",
            ExactSpelling = true, CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern IntPtr CreateJobObject(IntPtr attributes, string name);

        [DllImport("kernel32.dll", ExactSpelling = true, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool SetInformationJobObject(
            IntPtr job, int informationClass, ref ExtendedLimits information, uint length);

        [DllImport("kernel32.dll", EntryPoint = "CreateProcessW",
            ExactSpelling = true, CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool CreateProcess(
            string applicationName, StringBuilder commandLine, IntPtr processAttributes,
            IntPtr threadAttributes, [MarshalAs(UnmanagedType.Bool)] bool inheritHandles,
            uint creationFlags, IntPtr environment, string currentDirectory,
            ref StartupInfo startupInfo, out ProcessInformation processInformation);

        [DllImport("kernel32.dll", ExactSpelling = true, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);

        [DllImport("kernel32.dll", ExactSpelling = true, SetLastError = true)]
        private static extern uint ResumeThread(IntPtr thread);

        [DllImport("kernel32.dll", ExactSpelling = true, SetLastError = true)]
        private static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);

        [DllImport("kernel32.dll", ExactSpelling = true, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);

        [DllImport("kernel32.dll", ExactSpelling = true, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool TerminateProcess(IntPtr process, uint exitCode);

        [DllImport("kernel32.dll", ExactSpelling = true, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool CloseHandle(IntPtr handle);

        public static int Run(string cargoPath, string commandLine, string currentDirectory)
        {
            IntPtr job = IntPtr.Zero;
            ProcessInformation process = new ProcessInformation();
            try
            {
                job = CreateJobObject(IntPtr.Zero, null);
                if (job == IntPtr.Zero)
                    ThrowLastError("CreateJobObject");

                ExtendedLimits limits = new ExtendedLimits();
                limits.BasicLimitInformation.LimitFlags = KillOnJobClose;
                if (!SetInformationJobObject(
                    job, ExtendedLimitInformationClass, ref limits,
                    (uint)Marshal.SizeOf(typeof(ExtendedLimits))))
                    ThrowLastError("SetInformationJobObject");

                StartupInfo startup = new StartupInfo();
                startup.cb = (uint)Marshal.SizeOf(typeof(StartupInfo));
                if (!CreateProcess(
                    cargoPath, new StringBuilder(commandLine), IntPtr.Zero, IntPtr.Zero,
                    true, CreateSuspended, IntPtr.Zero, currentDirectory, ref startup,
                    out process))
                    ThrowLastError("CreateProcess");

                if (!AssignProcessToJobObject(job, process.hProcess))
                {
                    int error = Marshal.GetLastWin32Error();
                    TerminateAndWait(process.hProcess);
                    throw new Win32Exception(error, "AssignProcessToJobObject failed");
                }
                if (ResumeThread(process.hThread) == uint.MaxValue)
                {
                    int error = Marshal.GetLastWin32Error();
                    TerminateAndWait(process.hProcess);
                    throw new Win32Exception(error, "ResumeThread failed");
                }
                if (WaitForSingleObject(process.hProcess, Infinite) != WaitObject0)
                    ThrowLastError("WaitForSingleObject");

                uint exitCode;
                if (!GetExitCodeProcess(process.hProcess, out exitCode))
                    ThrowLastError("GetExitCodeProcess");
                return unchecked((int)exitCode);
            }
            catch
            {
                if (process.hProcess != IntPtr.Zero)
                    TerminateAndWait(process.hProcess);
                throw;
            }
            finally
            {
                if (process.hThread != IntPtr.Zero)
                    CloseHandle(process.hThread);
                if (process.hProcess != IntPtr.Zero)
                    CloseHandle(process.hProcess);
                // The last Job handle kills descendants left behind by cargo or its tests.
                if (job != IntPtr.Zero)
                    CloseHandle(job);
            }
        }

        private static void ThrowLastError(string operation)
        {
            throw new Win32Exception(Marshal.GetLastWin32Error(), operation + " failed");
        }

        private static void TerminateAndWait(IntPtr process)
        {
            TerminateProcess(process, 1);
            WaitForSingleObject(process, Infinite);
        }
    }
}
'@

if (-not ('WindowsSpawn.Tools.MutationJob' -as [type])) {
    Add-Type -TypeDefinition $nativeSource -Language CSharp
}

function ConvertTo-WindowsCommandLineArgument {
    param([AllowEmptyString()][string] $Argument)

    if ($Argument.Length -gt 0 -and $Argument -notmatch '[\s"]') {
        return $Argument
    }

    $quoted = [Text.StringBuilder]::new()
    [void] $quoted.Append([char] 0x22)
    $backslashes = 0
    foreach ($character in $Argument.ToCharArray()) {
        if ($character -eq [char] 0x5c) {
            $backslashes++
            continue
        }
        $copies = if ($character -eq [char] 0x22) {
            2 * $backslashes + 1
        } else {
            $backslashes
        }
        for ($index = 0; $index -lt $copies; $index++) {
            [void] $quoted.Append([char] 0x5c)
        }
        $backslashes = 0
        [void] $quoted.Append($character)
    }
    for ($index = 0; $index -lt (2 * $backslashes); $index++) {
        [void] $quoted.Append([char] 0x5c)
    }
    [void] $quoted.Append([char] 0x22)
    return $quoted.ToString()
}

$outputPath = [Environment]::GetEnvironmentVariable('CARGO_MUTANTS_OUTPUT')
if ([string]::IsNullOrWhiteSpace($outputPath)) {
    $runsDirectory = Join-Path $PWD.ProviderPath 'target\mutants\runs'
    [void] [IO.Directory]::CreateDirectory($runsDirectory)
    $runName = '{0}-{1}' -f [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ'), $PID
    $outputPath = Join-Path $runsDirectory $runName
    $env:CARGO_MUTANTS_OUTPUT = $outputPath
}
Write-Host "cargo-mutants output: $outputPath"

$cargo = Get-Command 'cargo.exe' -CommandType Application -ErrorAction Stop |
    Select-Object -First 1
$arguments = @($cargo.Path, 'mutants') + @($MutantsArguments)
$commandLine = ($arguments | ForEach-Object {
        ConvertTo-WindowsCommandLineArgument -Argument $_
    }) -join ' '

try {
    $exitCode = [WindowsSpawn.Tools.MutationJob]::Run(
        $cargo.Path,
        $commandLine,
        $PWD.ProviderPath)
} catch {
    Write-Error -ErrorRecord $_
    exit 1
}

exit $exitCode
