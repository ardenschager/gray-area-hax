/**
 * Specifies an envMap on an entity, without replacing any existing material
 * properties.
 */

 AFRAME.registerComponent('stomach', {
  
    init: function () {
        this.el.addEventListener('object3dset', this.setMaterialProps.bind(this));  
    },

    setMaterialProps: function() {
        const mesh = this.el.getObject3D('mesh');
        if (!mesh) return;
        mesh.traverse(function (node) {
            if (node.material) {
                // Set color from position
                let pos = document.querySelector('a-scene').camera.el.object3D.position; 
                let r = 0.2 * Math.max(pos.x, pos.y) + 0.5;
                let g = 0.2 * Math.sin(pos.z) + 0.35;
                let b = 0.2 * Math.cos(pos.x * 10 + pos.y * 10) * 0.5;
                node.material.color = new THREE.Color(r, g, b);
                // set properties
                node.material.side = THREE.DoubleSide;
                node.material.roughness = 0.5;
            }
        });
    },

    tick: function (t, dt) {
        const mesh = this.el.getObject3D('mesh');
        if (!mesh) return;
        mesh.traverse(function (node) {
            if (node.material) {
                // Set color from position
                let pos = document.querySelector('a-scene').camera.el.object3D.position; 
                let r = 0.2 * Math.max(pos.x, pos.y) + 0.35;
                let g = 0.2 * Math.sin(pos.z * 0.2) + 0.25;
                let b = 0.1 * Math.cos(pos.x + pos.y) + 0.3;
                node.material.color = new THREE.Color(r, g, b);
            }
        });
    }
});