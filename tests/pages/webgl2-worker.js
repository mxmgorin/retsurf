// Echoes the type it received, so the page can tell a delivered message from a
// structured-clone failure on the sending side.
onmessage = function (e) {
  var data = e.data;
  postMessage(data && data.constructor ? data.constructor.name : String(data));
};
