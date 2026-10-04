// Extracted from cstimer's twisty.js — the polygon-to-mesh triangulator class
THREE.Ploy = function(points) {
  var tridata = [];
  for (var i = 0; i < points.length; i++) {
    tridata.push({x: points[i][0], y: points[i][1]});
  }
  var myTriangulator = new PNLTRI.Triangulator();
  var triangList = myTriangulator.triangulate_polygon([tridata]);

  THREE.Geometry.call(this);
  for (var i = 0; i < points.length; i++) {
    this.vertices.push(new THREE.Vertex(new THREE.Vector3(points[i][0], points[i][1], 0)));
  }
  for (var i = 0; i < triangList.length; i++) {
    var tri = triangList[i];
    this.faces.push(new THREE.Face3(tri[0], tri[1], tri[2]));
    var mask = 0;
    for (var j = 0; j < 3; j++) {
      var gap = tri[j] - tri[(j + 1) % 3];
      if (gap != -1 && gap != points.length - 1) {
        mask |= 1 << j;
      }
    }
    this.faces[i].innerLineMask = mask;
  }
  this.computeCentroids();
  this.computeFaceNormals();
};
THREE.Ploy.prototype = new THREE.Geometry;
THREE.Ploy.prototype.constructor = THREE.Ploy;

var twistyjs = window.twistyjs || {};
twistyjs.axify = function(v1, v2, v3) {
  var ax = new THREE.Matrix4();
  ax.set(
    v1.x, v2.x, v3.x, 0,
    v1.y, v2.y, v3.y, 0,
    v1.z, v2.z, v3.z, 0,
    0, 0, 0, 1
  );
  return ax;
};
window.twistyjs = twistyjs;