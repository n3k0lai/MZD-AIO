function systemApp() {}
systemApp.prototype._displayedDisclaimer = function()
{
    framework.sendEventToMmui("system", "DisposeIntroVideo");
};
