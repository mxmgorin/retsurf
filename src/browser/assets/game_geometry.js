// What game_scaling.js last reported, or an empty array before it fits a game.
(function () {
    const scaling = document.documentElement[Symbol.for("retsurf.gameScaling")];
    return (scaling && scaling.game) || [];
})()
