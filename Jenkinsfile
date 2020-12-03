/**  
* Copyright Robert Bosch GmbH, 2020. Part of the Eclipse Kuksa Project.
*
* All rights reserved. This configuration file is provided to you under the
* terms and conditions of the Eclipse Distribution License v1.0 which
* accompanies this distribution, and is available at
* http://www.eclipse.org/org/documents/edl-v10.php
*
**/

node('docker') {
    checkout scm
    stage('Prepare') {
        sh '''
            git submodule update --init
            mkdir -p artifacts
            rm -rf ./artifacts/*
            '''
        }
        def versiontag=sh(returnStdout: true, script: "git tag --contains | head -n 1").trim()
        if (versiontag == "") { //not tagged, using commit
            versiontag = sh(returnStdout: true, script: "git rev-parse --short HEAD").trim()
        }
        echo "Using versiontag ${versiontag} for images";
    stage('Build') {
        parallel {
            stage('arm64') {
                sh "docker buildx build --platform=linux/arm64 -f ./docker/Dockerfile -t arm64/kuksa-val:${versiontag} --output type=docker,dest=./artifacts/kuksa-val-${versiontag}-arm64.tar ."
    	
            }
            stage('amd64') {
                sh "docker buildx build --platform=linux/amd64 -f ./docker/Dockerfile -t amd64/kuksa-val:${versiontag} --output type=docker,dest=./artifacts/kuksa-val-${versiontag}-amd64.tar ."
                sh "docker build -t kuksa-val-dev-ubuntu20.04:${versiontag} -f docker/Dockerfile.dev ."
            }
        }
    }
        stage('Compress') {
            sh 'ls artifacts'
            sh 'cd artifacts && xz -T 0 ./*.tar'
			/*sh '''
            sudo docker save $(sudo docker images --filter "reference=amd64/kuksa-val*"  --format "{{.Repository}}:{{.Tag}}" | head -1) | xz -T 0 > artifacts/kuksa-val-amd64.tar.xz
			sudo docker save $(sudo docker images --filter "reference=arm64/kuksa-val*"  --format "{{.Repository}}:{{.Tag}}" | head -1) | xz -T 0 > artifacts/kuksa-val-arm64.tar.xz
			sudo docker save kuksa-val-dev:ubuntu20.04 | xz -T 0 > artifacts/kuksa-val-dev-ubuntu20.04.tar.xz
            '''*/
        }
        stage ('Archive') {
            archiveArtifacts artifacts: 'artifacts/*.xz' 
        }
    
    /*
    docker.image('kuksa-val-dev:ubuntu20.04').inside("-v /var/run/docker.sock:/var/run/docker.sock"){ 
        stage('Test') {
            sh '''
                cd /kuksa.val/build
                # TODO and you may need sudo right for testing ctest --build-config Debug --output-on-failure --parallel 8
            '''
        }
    }*/
}
