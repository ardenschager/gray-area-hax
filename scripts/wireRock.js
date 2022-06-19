/**
 * Specifies an envMap on an entity, without replacing any existing material
 * properties.
 */

const wireRockVert = `

    precision mediump float;
    varying vec2 v_UV;
    varying vec4 v_Pos;
    varying vec3 v_VertPos;

    uniform float u_Time;
    //	Simplex 3D Noise 
    //	by Ian McEwan, Ashima Arts
    //
    vec4 permute(vec4 x){return mod(((x*34.0)+1.0)*x, 289.0);}
    vec4 taylorInvSqrt(vec4 r){return 1.79284291400159 - 0.85373472095314 * r;}

    float snoise(vec3 v){ 
        const vec2  C = vec2(1.0/6.0, 1.0/3.0) ;
        const vec4  D = vec4(0.0, 0.5, 1.0, 2.0);

        // First corner
        vec3 i  = floor(v + dot(v, C.yyy) );
        vec3 x0 =   v - i + dot(i, C.xxx) ;

        // Other corners
        vec3 g = step(x0.yzx, x0.xyz);
        vec3 l = 1.0 - g;
        vec3 i1 = min( g.xyz, l.zxy );
        vec3 i2 = max( g.xyz, l.zxy );

        //  x0 = x0 - 0. + 0.0 * C 
        vec3 x1 = x0 - i1 + 1.0 * C.xxx;
        vec3 x2 = x0 - i2 + 2.0 * C.xxx;
        vec3 x3 = x0 - 1. + 3.0 * C.xxx;

        // Permutations
        i = mod(i, 289.0 ); 
        vec4 p = permute( permute( permute( 
                    i.z + vec4(0.0, i1.z, i2.z, 1.0 ))
                + i.y + vec4(0.0, i1.y, i2.y, 1.0 )) 
                + i.x + vec4(0.0, i1.x, i2.x, 1.0 ));

        // Gradients
        // ( N*N points uniformly over a square, mapped onto an octahedron.)
        float n_ = 1.0/7.0; // N=7
        vec3  ns = n_ * D.wyz - D.xzx;

        vec4 j = p - 49.0 * floor(p * ns.z *ns.z);  //  mod(p,N*N)

        vec4 x_ = floor(j * ns.z);
        vec4 y_ = floor(j - 7.0 * x_ );    // mod(j,N)

        vec4 x = x_ *ns.x + ns.yyyy;
        vec4 y = y_ *ns.x + ns.yyyy;
        vec4 h = 1.0 - abs(x) - abs(y);

        vec4 b0 = vec4( x.xy, y.xy );
        vec4 b1 = vec4( x.zw, y.zw );

        vec4 s0 = floor(b0)*2.0 + 1.0;
        vec4 s1 = floor(b1)*2.0 + 1.0;
        vec4 sh = -step(h, vec4(0.0));

        vec4 a0 = b0.xzyw + s0.xzyw*sh.xxyy ;
        vec4 a1 = b1.xzyw + s1.xzyw*sh.zzww ;

        vec3 p0 = vec3(a0.xy,h.x);
        vec3 p1 = vec3(a0.zw,h.y);
        vec3 p2 = vec3(a1.xy,h.z);
        vec3 p3 = vec3(a1.zw,h.w);

        //Normalise gradients
        vec4 norm = taylorInvSqrt(vec4(dot(p0,p0), dot(p1,p1), dot(p2, p2), dot(p3,p3)));
        p0 *= norm.x;
        p1 *= norm.y;
        p2 *= norm.z;
        p3 *= norm.w;

        // Mix final noise value
        vec4 m = max(0.6 - vec4(dot(x0,x0), dot(x1,x1), dot(x2,x2), dot(x3,x3)), 0.0);
        m = m * m;
        return 42.0 * dot( m*m, vec4( dot(p0,x0), dot(p1,x1), 
                                        dot(p2,x2), dot(p3,x3) ) );
    }

    void main() {
        v_UV = uv + vec2(0.03 * snoise(position * u_Time * 0.000002));
        v_VertPos = position;
        vec3 pos = position;//  + normal * ;
        v_Pos = projectionMatrix * modelViewMatrix * vec4( pos, 1.0 );
        gl_Position = v_Pos;
    }
`;

const wireRockFrag = `
    precision mediump float;

    varying vec2 v_UV;
    varying vec3 v_VertPos;
    varying vec4 v_Pos;

    uniform vec3 u_CameraPos;
    uniform float u_Time;
    uniform vec3 u_Color;
    uniform sampler2D u_AlbedoTex;

    // This receives the opacity value from the schema, which becomes a number.
    uniform float u_Opacity;

    void main () {
        float uvVal = v_UV.x * v_UV.y;
        vec4 albedoCol = texture2D(u_AlbedoTex, v_UV);
        float albedoVal = max(max(albedoCol.r, albedoCol.g), albedoCol.b);
        albedoCol.rg *= albedoVal * uvVal;
        albedoCol.rg += 0.3;
        albedoCol.rg = min(vec2(1.), albedoCol.rg);
        albedoCol.r += 0.1;
        albedoCol.b = min(1., albedoCol.b * 0.6 + 0.8);
        albedoCol.rgb -= 0.5 * sin(v_VertPos.y * 0.05 + 10.) + 0.5;
        gl_FragColor = vec4(albedoCol.rgb, 0.9);
    }
`;

const albedoTexture = new THREE.TextureLoader().load( "images/liftAlbedo.jpg" );

let wireRockMaterial = new THREE.ShaderMaterial( {
	uniforms: {
		u_Time: { value: 0.0 },
        u_CameraPos: {value: new THREE.Vector3() },
        u_AlbedoTex: {value: albedoTexture},
	},
	vertexShader: wireRockVert,
	fragmentShader: wireRockFrag,
    transparent: true,
} );

AFRAME.registerComponent('wire-rock', {
  
    init: function () {
        // For changing the material for a gltf model, 
        // you have to add a callback to this event
        this.el.addEventListener('object3dset', this.setMaterialProps.bind(this));  
    },

    setMaterialProps: function() {
        const mesh = this.el.getObject3D('mesh');
        if (!mesh) return;
        mesh.traverse(function (node) {
            if (node.material) {
                node.material.side = THREE.DoubleSide;
                node.material = wireRockMaterial.clone();
                this.material = node.material;
            }
        });
    },

    tick: function (t, dt) {
        console.log("asdf", "asdf");
        // Have to get the mesh every tick, sometimes this.material gets unset
        const mesh = this.el.getObject3D('mesh');
        if (!mesh) return;
        mesh.traverse(function (node) {
            if (node.material) {
                var pos = document.querySelector('a-scene').camera.el.object3D.position; 
                this.material = node.material;
                // Set uniforms here
                node.material.uniforms.u_Time.value = t;
                node.material.uniforms.u_CameraPos.value = pos;
                node.material.uniforms.u_AlbedoTex.value = albedoTexture;
            }
        });
    },
});