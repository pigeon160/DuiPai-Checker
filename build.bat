@echo off
chcp 65001 >nul
REM ============================================================
REM  竞赛对拍机 - Windows 打包脚本（生成无控制台的单文件 exe）
REM  用法：双击运行，或在命令行执行 build.bat
REM  输出：dist\对拍机.exe
REM ============================================================
setlocal

echo [1/3] 生成应用图标 ...
python make_icon.py
if errorlevel 1 goto :err

echo [2/3] 检查 PyInstaller ...
python -m PyInstaller --version >nul 2>&1
if errorlevel 1 (
    echo      未安装 PyInstaller，正在安装（需要联网）...
    python -m pip install pyinstaller
    if errorlevel 1 goto :err
)

echo [3/3] 打包为无控制台单文件 exe ...
python -m PyInstaller --noconsole --onefile --clean --name 对拍机 --icon app.ico duipai.py
if errorlevel 1 goto :err

echo.
echo 打包完成：dist\对拍机.exe
goto :end

:err
echo.
echo 打包失败，请检查上面的错误信息。
exit /b 1

:end
endlocal
